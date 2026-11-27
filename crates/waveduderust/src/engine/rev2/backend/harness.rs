//! The unit tests' harness of the backends: a backend's kernels against the generic backend's on random spans, within
//! a few ulps of the magnitudes each value is computed from, and with guard cells around every output that no kernel
//! may touch. A backend's tests call [`nine_matches_generic`] and [`drp_match_generic`] with a value of it.

use super::generic::Generic;
use super::{Intensity, RowKernels, RowMedium};

/// A xorshift generator of values in [-1, 1).
struct Values(u32);

impl Values {
	fn next(&mut self) -> f32 {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 17;
		self.0 ^= self.0 << 5;
		(self.0 >> 8) as f32 / (1 << 23) as f32 - 1.0
	}

	fn take(&mut self, n: usize, f: impl Fn(f32) -> f32) -> Vec<f32> {
		(0..n).map(|_| f(self.next())).collect()
	}
}

/// Written around the span; no kernel may touch it.
const GUARD: f32 = 1234.5;
/// Guard cells on each side of the outputs.
const SIDE: usize = 9;

/// `values` between two runs of [`GUARD`] cells.
fn guarded(values: &[f32]) -> Vec<f32> {
	[&[GUARD; SIDE][..], values, &[GUARD; SIDE]].concat()
}

/// The outputs of a 9-point kernel: the next field and the stages, each guarded.
struct Outputs {
	next: Vec<f32>,
	s1: Vec<f32>,
	s2: Vec<f32>,
}

struct Case {
	rows: [Vec<f32>; 3],
	c2: Vec<f32>,
	d: Vec<f32>,
	before: Vec<f32>,
	s1: Vec<f32>,
	s2: Vec<f32>,
	uniform: bool,
	stages: bool,
}

impl Case {
	fn new(values: &mut Values, len: usize, uniform: bool, stages: bool) -> Self {
		let rows = std::array::from_fn(|_| values.take(len + 2, |v| v));
		let (c2, d) = if uniform {
			(vec![0.3 + 0.1 * values.next(); len.max(1)], vec![0.9 + 0.05 * values.next(); len.max(1)])
		} else {
			(values.take(len, |v| 0.3 + 0.2 * v), values.take(len, |v| 0.9 + 0.1 * v))
		};
		let before = values.take(len, |v| v);
		let (s1, s2) = (values.take(len, f32::abs), values.take(len, f32::abs));
		Self { rows, c2, d, before, s1, s2, uniform, stages }
	}

	fn run(&self, kernels: impl RowKernels) -> Outputs {
		let len = self.before.len();
		let mut out = Outputs { next: guarded(&self.before), s1: guarded(&self.s1), s2: guarded(&self.s2) };
		let medium = if self.uniform {
			RowMedium::Uniform { c2: self.c2[0], d: self.d[0] }
		} else {
			RowMedium::Planes { c2: &self.c2, d: &self.d }
		};
		let stages = self.stages.then(|| Intensity {
			s1: &mut out.s1[SIDE..][..len],
			s2: &mut out.s2[SIDE..][..len],
			alpha: 0.95,
		});
		let rows = [&self.rows[0][..], &self.rows[1][..], &self.rows[2][..]];
		kernels.nine(rows, medium, &mut out.next[SIDE..][..len], stages);
		out
	}

	/// Checks `got` against the generic backend's `want`: within a few ulps of the magnitudes each value is
	/// computed from, and the guards untouched.
	fn check(&self, got: &Outputs, want: &Outputs, what: &str) {
		let len = self.before.len();
		for (name, g) in [("next", &got.next), ("s1", &got.s1), ("s2", &got.s2)] {
			assert_eq!(g.len(), len + 2 * SIDE);
			let guards = g[..SIDE].iter().chain(&g[SIDE + len..]);
			assert!(guards.into_iter().all(|&v| v == GUARD), "{what}: {name} written outside the span");
		}
		let ulps = 8.0 * f32::EPSILON;
		for x in 0..len {
			let field =
				(0..3).flat_map(|r| self.rows[r][x..x + 3].iter()).fold(self.before[x].abs(), |m, v| m.max(v.abs()));
			let (next, s1) = (field, self.s1[x] + field * field);
			let s2 = self.s2[x] + s1;
			let at = SIDE + x;
			for (name, g, w, scale) in [
				("next", got.next[at], want.next[at], next),
				("s1", got.s1[at], want.s1[at], s1),
				("s2", got.s2[at], want.s2[at], s2),
			] {
				assert!((g - w).abs() <= ulps * scale, "{what}: {name} at {x} is {g}, the generic backend's {w}");
			}
		}
	}
}

/// Asserts `kernels`' 9-point kernel computes the generic backend's values within a few ulps and writes only its
/// span, on spans of 0 to 39 cells and two long ones, on either medium, with and without the stages.
pub(crate) fn nine_matches_generic(kernels: impl RowKernels) {
	let mut values = Values(0x9e37_79b9);
	for len in (0..40).chain([1000, 1003]) {
		for (uniform, stages) in [(false, false), (false, true), (true, false), (true, true)] {
			let case = Case::new(&mut values, len, uniform, stages);
			let what = format!("{len} cells, uniform {uniform}, stages {stages}");
			case.check(&case.run(kernels), &case.run(Generic), &what);
		}
	}
}

/// The DRP weights a0..a3 of the 4th-order stencil.
fn weights() -> [f32; 4] {
	crate::engine::stencil::StencilCoefficients::of(crate::model::Stencil::FourthOrder).axis.map(|a| a as f32)
}

/// The inputs of the DRP kernels on a span of `len` cells: seven rows `spacing` values apart in one buffer (each
/// `len + 6` values long), the span's field, medium, previous values and stages.
struct DrpCase {
	buf: Vec<f32>,
	spacing: usize,
	u: Vec<f32>,
	c2: Vec<f32>,
	d: Vec<f32>,
	before: Vec<f32>,
	s1: Vec<f32>,
	s2: Vec<f32>,
	uniform: bool,
	stages: bool,
}

impl DrpCase {
	fn new(values: &mut Values, len: usize, spacing: usize, uniform: bool, stages: bool) -> Self {
		let buf = values.take(6 * spacing + len + 6, |v| v);
		let (c2, d) = if uniform {
			(vec![0.3 + 0.1 * values.next(); len.max(1)], vec![0.9 + 0.05 * values.next(); len.max(1)])
		} else {
			(values.take(len, |v| 0.3 + 0.2 * v), values.take(len, |v| 0.9 + 0.1 * v))
		};
		let (u, before) = (values.take(len, |v| v), values.take(len, |v| v));
		let (s1, s2) = (values.take(len, f32::abs), values.take(len, f32::abs));
		Self { buf, spacing, u, c2, d, before, s1, s2, uniform, stages }
	}

	fn rows(&self) -> [&[f32]; 7] {
		let len = self.u.len();
		std::array::from_fn(|k| &self.buf[k * self.spacing..][..len + 6])
	}

	fn medium(&self) -> RowMedium<'_> {
		if self.uniform {
			RowMedium::Uniform { c2: self.c2[0], d: self.d[0] }
		} else {
			RowMedium::Planes { c2: &self.c2, d: &self.d }
		}
	}

	/// The largest magnitude a value of `L` of cell `x` is computed from: the taps times the weights' sum.
	fn lap_scale(&self, x: usize) -> f32 {
		let a = weights();
		let taps = self.rows().iter().flat_map(|row| row[x..x + 7].iter()).fold(0f32, |m, v| m.max(v.abs()));
		8.0 * (a[1].abs() + a[2].abs() + a[3].abs()) * taps
	}

	fn lap(&self, kernels: impl RowKernels) -> Vec<f32> {
		let len = self.u.len();
		let mut w = guarded(&vec![0.0; len]);
		kernels.lap(self.rows(), &weights(), self.medium(), &mut w[SIDE..][..len]);
		w
	}

	fn outer(&self, kernels: impl RowKernels) -> Outputs {
		let len = self.u.len();
		let mut out = Outputs { next: guarded(&self.before), s1: guarded(&self.s1), s2: guarded(&self.s2) };
		let stages = self.stages.then(|| Intensity {
			s1: &mut out.s1[SIDE..][..len],
			s2: &mut out.s2[SIDE..][..len],
			alpha: 0.95,
		});
		kernels.outer(&self.u, self.rows(), &weights(), self.medium(), &mut out.next[SIDE..][..len], stages);
		out
	}
}

/// Asserts no value of `got` outside its `len` span differs from [`GUARD`].
fn guards_kept(got: &[f32], len: usize, what: &str) {
	assert_eq!(got.len(), len + 2 * SIDE);
	let guards = got[..SIDE].iter().chain(&got[SIDE + len..]);
	assert!(guards.into_iter().all(|&v| v == GUARD), "{what}: written outside the span");
}

/// Asserts `kernels`' DRP kernels compute the generic backend's values within a few ulps and write only their span,
/// on spans of 0 to 39 cells and two long ones, rows at two strides, on either medium, with and without the stages.
pub(crate) fn drp_match_generic(kernels: impl RowKernels) {
	let mut values = Values(0x2545_f491);
	let ulps = 8.0 * f32::EPSILON;
	for len in (0..40).chain([1000, 1003]) {
		// Rows a plane's stride apart, at two strides, as the engine hands them over (its rows come from one plane
		// or one buffer).
		for spacing in [len + 6, len + 17] {
			for (uniform, stages) in [(false, false), (false, true), (true, false), (true, true)] {
				let case = DrpCase::new(&mut values, len, spacing, uniform, stages);
				let what = format!("{len} cells {spacing} apart, uniform {uniform}, stages {stages}");
				let (got, want) = (case.lap(kernels), case.lap(Generic));
				guards_kept(&got, len, &format!("lap, {what}"));
				for x in 0..len {
					let (g, w, scale) =
						(got[SIDE + x], want[SIDE + x], case.lap_scale(x) * case.c2[x.min(case.c2.len() - 1)]);
					assert!((g - w).abs() <= ulps * scale, "lap, {what}: {g} at {x}, the generic backend's {w}");
				}
				let (got, want) = (case.outer(kernels), case.outer(Generic));
				for (name, g) in [("next", &got.next), ("s1", &got.s1), ("s2", &got.s2)] {
					guards_kept(g, len, &format!("outer {name}, {what}"));
				}
				for x in 0..len {
					let c2 = case.c2[x.min(case.c2.len() - 1)];
					let wc = case.rows()[3][x + 3].abs();
					let next = case.u[x].abs() + case.before[x].abs() + wc + c2 * case.lap_scale(x);
					let s1 = case.s1[x] + next * next;
					let s2 = case.s2[x] + s1;
					let at = SIDE + x;
					for (name, g, w, scale) in [
						("next", got.next[at], want.next[at], next),
						("s1", got.s1[at], want.s1[at], s1),
						("s2", got.s2[at], want.s2[at], s2),
					] {
						assert!(
							(g - w).abs() <= ulps * scale,
							"outer, {what}: {name} at {x} is {g}, the generic backend's {w}"
						);
					}
				}
			}
		}
	}
}
