//! Guided modes of a straight slab waveguide as this project's engine discretises it.
//!
//! The engine advances `u+ = 2u - u- + B u` per sub-step, `B = A` (leapfrog) or `A + A^2/12` (the
//! modified equation), `A = (c2/S^2) L`, S sub-steps a step, c2 = 1/n^2 (engine/stencil.rs). A mode
//! `u = phi_j exp(i(beta x - omega t))` (x along the guide, rows j across) is an eigenvector of A; its
//! eigenvalue `-s` follows from the phase per sub-step `omega/S` by the scheme
//! ([`TimeScheme::symbol_of_phase`]), so `L phi + Om n_j^2 phi = 0` with `Om = S^2 s`:
//!
//! ```text
//! leapfrog:              Om = 4 S^2 sin^2(omega / 2S)
//! modified equation:     Om = 6 S^2 (1 - sqrt(1 - (4/3) sin^2(omega / 2S)))
//! ```
//!
//! - Axis stencils (cross weight 0; the DRP 7-point and the Taylor 4th order): `L = D_x + D_y`,
//!   `D_x exp(i beta x) = -p(beta) exp(i beta x)` (p the axis symbol), so
//!   `(D_y + diag(Om n^2)) phi = p(beta) phi`, a symmetric banded eigenproblem (bandwidth the
//!   stencil's reach) solved by shifted inverse iteration with a banded Cholesky factor; then
//!   `beta = p^-1(eigenvalue)`.
//! - 9-point (`D3x + D3y + D3x D3y / 6`): with `c = cos(beta)`, `D3x -> -2(1 - c)`, so
//!   `(a D3y + diag(Om n^2)) phi = 2(1 - c) phi`, `a = 1 - cross 2(1 - c)`. The largest eigenvalue
//!   `lambda(c)` of the left side minus `2(1 - c)` rises with c, so c is found by a safeguarded
//!   Newton iteration (Hellmann-Feynman slope), then `beta = acos(c)`.
//!
//! S is the engine's sub-step count ([`sub_steps`], the CFL rule of the stencil) for a scene whose
//! fastest pixel has index 1; the original fixed S = 3, leapfrog and the Taylor 4th order
//! ([`ModeSolver::original`]). All lengths in cells, f64 throughout.

use crate::engine::stencil::{StencilCoefficients, TimeScheme};
use crate::engine::substeps::sub_steps;
use crate::model::{Simulation, Stencil};

/// How a painted row's index comes from its samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowMean {
	/// The root of the mean n^2, the painter's area-weighted permittivity.
	Permittivity,
	/// The mean n, the original's rows.
	Index,
}

/// The index profile sampled as a painter paints rows: row j spans `[first + j, first + j + 1)`,
/// its index the `mean` of 4 samples rounded half-even to 2 decimals.
pub fn rows(first: i64, count: usize, mean: RowMean, profile: impl Fn(f64) -> f64) -> Vec<f64> {
	(0..count)
		.map(|j| {
			let base = (first + j as i64) as f64;
			let samples = (0..4).map(|k| profile(base + (k as f64 + 0.5) / 4.0));
			let n = match mean {
				RowMean::Permittivity => (samples.map(|n| n * n).sum::<f64>() / 4.0).sqrt(),
				RowMean::Index => samples.sum::<f64>() / 4.0,
			};
			crate::scene::shapes::round_digits(n, 2)
		})
		.collect()
}

/// A guided mode: propagation constant and what the cladding decay needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
	/// Radians per cell along the guide.
	pub beta: f64,
	/// The transverse eigenvalue: the axis symbol `p(beta)`.
	pub eigen: f64,
	/// The transverse operator's scale: 1 (axis stencils) or `1 - cross 2(1 - cos beta)` (9-point).
	pub scale: f64,
}

impl Mode {
	/// Decay constant (1/cell) in a cladding of index `n`: `sqrt(max(1e-9, (eigen - Om n^2)/scale))`.
	pub fn cladding_decay(&self, om: f64, n: f64) -> f64 {
		((self.eigen - om * n * n) / self.scale).max(1e-9).sqrt()
	}
}

/// The mode solver for one stencil, time scheme and sub-step count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeSolver {
	pub coefficients: StencilCoefficients,
	pub scheme: TimeScheme,
	pub sub_steps: u32,
	/// How the guide's rows are painted.
	pub rows: RowMean,
}

impl Eq for ModeSolver {}

impl std::hash::Hash for ModeSolver {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.coefficients.axis.map(f64::to_bits).hash(state);
		self.coefficients.cross.to_bits().hash(state);
		(self.scheme, self.sub_steps, self.rows).hash(state);
	}
}

impl ModeSolver {
	/// The engine's coefficients and time scheme for `stencil` at `sub_steps`.
	pub fn new(stencil: Stencil, sub_steps: u32) -> Self {
		Self {
			coefficients: StencilCoefficients::of(stencil),
			scheme: TimeScheme::of(stencil),
			sub_steps,
			rows: RowMean::Permittivity,
		}
	}

	/// The original's solver (spec 22): Taylor 4th order, leapfrog, S = 3, rows of mean n.
	pub fn original() -> Self {
		Self {
			coefficients: StencilCoefficients::TAYLOR_FOURTH,
			scheme: TimeScheme::Leapfrog,
			sub_steps: 3,
			rows: RowMean::Index,
		}
	}

	/// The engine's stencil with the sub-steps of a scene whose fastest pixel has index 1.
	pub fn for_simulation(sim: &Simulation) -> Self {
		Self::new(sim.stencil, sub_steps(sim.stencil, 1.0))
	}

	/// `Om = S^2 s(omega / S)`, `omega = 2 pi / lam` (lam in cells at index 1); see the module docs.
	pub fn omega_term(&self, lam: f64) -> f64 {
		let s = self.sub_steps as f64;
		s * s * self.scheme.symbol_of_phase(std::f64::consts::TAU / lam / s)
	}

	/// The fundamental mode of a symmetry class (even or odd about the array's middle), or None
	/// when it is not guided against the edge rows.
	pub fn beta(&self, index: &[f64], om: f64, odd: bool) -> Option<Mode> {
		let potential: Vec<f64> = index.iter().map(|n| om * n * n).collect();
		let c = &self.coefficients;
		if c.cross == 0.0 { banded_beta(c, &potential, odd) } else { nine_point_beta(c.cross, &potential, odd) }
	}

	/// A single guide: core `w` (cells) of `n_core`, cladding `n_clad` out to `w/2 + reach`
	/// (`reach = cl > 0 ? cl : 2 lam`), index 1 for 8 rows beyond.
	pub fn single(&self, lam: f64, om: f64, n_core: f64, n_clad: f64, w: i64, cl: i64) -> Option<Mode> {
		let reach = if cl > 0 { cl as f64 } else { 2.0 * lam };
		let core = w as f64 / 2.0;
		let half = (core + reach + 8.0).ceil() as i64;
		let profile = rows(-half, 2 * half as usize, self.rows, |y| {
			if y.abs() <= core {
				n_core
			} else if y.abs() <= core + reach {
				n_clad
			} else {
				1.0
			}
		});
		self.beta(&profile, om, false)
	}

	/// Coupling constant (rad/cell) of two guides `dist` apart centre to centre: `(beta_even - beta_odd)/2`, 0 if either is unguided.
	pub fn kappa_at(&self, dist: f64, om: f64, n_core: f64, n_clad: f64, w: i64, cl: i64) -> f64 {
		let core = w as f64 / 2.0;
		let outer = core + cl as f64;
		let half = (dist / 2.0 + outer + 8.0).ceil() as i64;
		let profile = rows(-half, 2 * half as usize, self.rows, |y| {
			let e = (y - dist / 2.0).abs().min((y + dist / 2.0).abs());
			if e <= core {
				n_core
			} else if e <= outer {
				n_clad
			} else {
				1.0
			}
		});
		match (self.beta(&profile, om, false), self.beta(&profile, om, true)) {
			(Some(e), Some(o)) => ((e.beta - o.beta) / 2.0).max(0.0),
			_ => 0.0,
		}
	}

	/// Full-transfer coupling length `pi / (2 kappa)` (cells) of two guides `dist` apart centre to
	/// centre, as [`kappa_at`](Self::kappa_at); None when they do not couple. A 3 dB coupler is half of it.
	pub fn coupling_length(&self, dist: f64, om: f64, n_core: f64, n_clad: f64, w: i64, cl: i64) -> Option<f64> {
		let kappa = self.kappa_at(dist, om, n_core, n_clad, w, cl);
		(kappa > 0.0).then(|| std::f64::consts::FRAC_PI_2 / kappa)
	}
}

fn start_vector(n: usize, odd: bool) -> Vec<f64> {
	(0..n)
		.map(|j| {
			let t = (j as f64 + 0.5) / n as f64 - 0.5;
			if odd { t } else { 1.0 - t * t }
		})
		.collect()
}

/// Projects onto the symmetry class about the array's middle and normalises.
fn symmetrise(x: &mut [f64], odd: bool) {
	let n = x.len();
	for j in 0..n / 2 {
		let (a, b) = (x[j], x[n - 1 - j]);
		x[j] = if odd { (a - b) / 2.0 } else { (a + b) / 2.0 };
		x[n - 1 - j] = if odd { -x[j] } else { x[j] };
	}
	if n % 2 == 1 && odd {
		x[n / 2] = 0.0;
	}
	let norm = x.iter().map(|v| v * v).sum::<f64>().sqrt();
	x.iter_mut().for_each(|v| *v /= norm);
}

/// Spec 22, for any axis stencil: banded Cholesky of `sigma I - A`, `A = D_y + diag(p)`, shifted
/// inverse iteration, Rayleigh quotient.
fn banded_beta(coefficients: &StencilCoefficients, p: &[f64], odd: bool) -> Option<Mode> {
	let n = p.len();
	let a = coefficients.axis;
	let band = coefficients.reach();
	let sigma = p.iter().copied().fold(0.0, f64::max) + 1e-9;
	// l[j][m] = L[j, j - m]
	let mut l = vec![[0.0f64; 4]; n];
	for j in 0..n {
		for m in (1..=band.min(j)).rev() {
			let k = j - m;
			let mut v = -a[m];
			for t in 1..=band - m {
				if t <= k {
					v -= l[j][m + t] * l[k][t];
				}
			}
			l[j][m] = v / l[k][0];
		}
		let off: f64 = (1..=band.min(j)).map(|m| l[j][m] * l[j][m]).sum();
		l[j][0] = (sigma - a[0] - p[j] - off).sqrt();
	}
	let mut x = start_vector(n, odd);
	let at = |x: &[f64], k: isize| if k >= 0 && (k as usize) < n { x[k as usize] } else { 0.0 };
	let mut eigen = f64::NAN;
	for _ in 0..400 {
		symmetrise(&mut x, odd);
		let mut next = 0.0;
		for j in 0..n {
			let k = j as isize;
			let ax = (1..=band).map(|m| a[m] * (at(&x, k - m as isize) + at(&x, k + m as isize))).sum::<f64>()
				+ (a[0] + p[j]) * x[j];
			next += x[j] * ax;
		}
		if (next - eigen).abs() < 1e-15 {
			eigen = next;
			break;
		}
		eigen = next;
		for j in 0..n {
			let s: f64 = x[j] - (1..=band.min(j)).map(|m| l[j][m] * x[j - m]).sum::<f64>();
			x[j] = s / l[j][0];
		}
		for j in (0..n).rev() {
			let s: f64 = x[j] - (1..=band).filter(|&m| j + m < n).map(|m| l[j + m][m] * x[j + m]).sum::<f64>();
			x[j] = s / l[j][0];
		}
	}
	let edge = p[0].max(p[n - 1]);
	(eigen > edge).then(|| Mode { beta: coefficients.axis_wavenumber(eigen), eigen, scale: 1.0 })
}

/// Largest eigenvalue in a symmetry class of `a D2 + diag(p)` (D2: -2 diagonal, 1 off, Dirichlet)
/// by shifted inverse iteration from `x`, which it leaves at the eigenvector; returns
/// `(lambda, x^T D2 x)`.
fn tridiagonal_top(p: &[f64], a: f64, odd: bool, x: &mut [f64]) -> (f64, f64) {
	let n = p.len();
	let sigma = p.iter().copied().fold(0.0, f64::max) + 1e-9;
	let (mut l0, mut l1) = (vec![0.0; n], vec![0.0; n]);
	for j in 0..n {
		if j >= 1 {
			l1[j] = -a / l0[j - 1];
		}
		l0[j] = (sigma - p[j] + 2.0 * a - l1[j] * l1[j]).sqrt();
	}
	let second = |x: &[f64]| {
		(0..n)
			.map(|j| {
				let left = if j >= 1 { x[j - 1] } else { 0.0 };
				let right = if j + 1 < n { x[j + 1] } else { 0.0 };
				x[j] * (left + right - 2.0 * x[j])
			})
			.sum::<f64>()
	};
	let mut lambda = f64::NAN;
	for _ in 0..400 {
		symmetrise(x, odd);
		let curvature = second(x);
		let next = a * curvature + x.iter().zip(p).map(|(v, q)| v * v * q).sum::<f64>();
		if (next - lambda).abs() < 1e-15 {
			return (next, curvature);
		}
		lambda = next;
		for j in 0..n {
			let s = x[j] - if j >= 1 { l1[j] * x[j - 1] } else { 0.0 };
			x[j] = s / l0[j];
		}
		for j in (0..n).rev() {
			let s = x[j] - if j + 1 < n { l1[j + 1] * x[j + 1] } else { 0.0 };
			x[j] = s / l0[j];
		}
	}
	symmetrise(x, odd);
	let curvature = second(x);
	(a * curvature + x.iter().zip(p).map(|(v, q)| v * v * q).sum::<f64>(), curvature)
}

/// 9-point: the root in c = cos(beta) of `f(c) = lambda(c) - 2(1 - c)`, `a(c) = 1 - cross 2(1 - c)`
/// (`(4 + 2c)/6` at cross 1/6), `f' = 2 cross (x^T D2 x) + 2 > 0`. Guided when `2(1 - c) > Om n_edge^2`.
fn nine_point_beta(cross: f64, p: &[f64], odd: bool) -> Option<Mode> {
	let n = p.len();
	let scale = |c: f64| 1.0 - cross * 2.0 * (1.0 - c);
	let mut x = start_vector(n, odd);
	let f = |c: f64, x: &mut [f64]| {
		let (lambda, curvature) = tridiagonal_top(p, scale(c), odd, x);
		(lambda - 2.0 * (1.0 - c), 2.0 * cross * curvature + 2.0)
	};
	let (mut lo, mut hi) = (-1.0f64, 1.0f64);
	if f(hi, &mut x).0 <= 0.0 || f(lo, &mut start_vector(n, odd)).0 >= 0.0 {
		return None;
	}
	let top = p.iter().copied().fold(0.0, f64::max);
	let mut c = (1.0 - top / 2.0).clamp(-0.5, 1.0 - 1e-12);
	for _ in 0..200 {
		let (value, slope) = f(c, &mut x);
		if value > 0.0 {
			hi = c;
		} else {
			lo = c;
		}
		let mut next = c - value / slope;
		if !(next > lo && next < hi) {
			next = (lo + hi) / 2.0;
		}
		let done = (next - c).abs() < 1e-15 || hi - lo < 1e-15;
		c = next;
		if done {
			break;
		}
	}
	let eigen = 2.0 * (1.0 - c);
	let edge = p[0].max(p[n - 1]);
	(eigen > edge).then(|| Mode { beta: c.clamp(-1.0, 1.0).acos(), eigen, scale: scale(c) })
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fourth() -> ModeSolver {
		ModeSolver::original()
	}

	#[test]
	fn axis_symbol_range() {
		let c = StencilCoefficients::TAYLOR_FOURTH;
		assert_eq!(c.axis_symbol(0.0), 0.0);
		assert!((c.axis_symbol(std::f64::consts::PI) - 16.0 / 3.0).abs() < 1e-12);
		assert!((c.axis_wavenumber(c.axis_symbol(0.7)) - 0.7).abs() < 1e-12);
	}

	#[test]
	fn modified_equation_omega_term_is_fourth_order() {
		let lam = 16.0;
		let k = std::f64::consts::TAU / lam;
		let leapfrog = ModeSolver { scheme: TimeScheme::Leapfrog, ..ModeSolver::new(Stencil::FourthOrder, 2) };
		let modified = ModeSolver::new(Stencil::FourthOrder, 2);
		assert!((modified.omega_term(lam) - k * k).abs() < 1e-4 * k * k);
		assert!((leapfrog.omega_term(lam) - k * k).abs() > 1e-3 * k * k);
	}

	#[test]
	fn drp_guide_tends_to_the_continuum() {
		let lam = 32.0;
		let s = ModeSolver::new(Stencil::FourthOrder, 2);
		let drp = s.single(lam, s.omega_term(lam), 1.6, 1.45, 10, 0).unwrap();
		let t = fourth();
		let taylor = t.single(lam, t.omega_term(lam), 1.6, 1.45, 10, 0).unwrap();
		assert!((drp.beta - taylor.beta).abs() < 2e-3 * taylor.beta, "{} vs {}", drp.beta, taylor.beta);
	}

	#[test]
	fn worked_example_default_gate() {
		let s = fourth();
		let om = s.omega_term(32.044);
		assert!((om - 0.0384336543).abs() < 1e-10, "{om}");
		let single = s.single(32.044, om, 2.55, 1.45, 6, 0).unwrap();
		assert!((single.beta - 0.41691857833).abs() < 1e-10, "{}", single.beta);
		assert!((single.eigen - 0.17376364604).abs() < 1e-10);
		assert!((single.cladding_decay(om, 1.45) - 0.30488832).abs() < 1e-7);
		let b = s.single(32.044, om, 2.55, 1.45, 6, 27).unwrap().beta;
		let bp = s.single(32.044, om, 2.45, 1.45, 6, 27).unwrap().beta;
		assert!((b - 0.41691857764).abs() < 1e-10, "{b}");
		assert!((bp - 0.39908083676).abs() < 1e-10, "{bp}");
		for (dist, kappa) in [(10.0, 0.0159001222), (12.0, 0.0085384994), (20.0, 0.0007358294)] {
			let k = s.kappa_at(dist, om, 2.55, 1.45, 6, 27);
			assert!((k - kappa).abs() < 1e-10, "{dist}: {k}");
		}
	}

	#[test]
	fn rows_take_the_painters_permittivity() {
		let core = |y: f64| if y.abs() <= 2.6 { 2.0 } else { 1.0 };
		let (engine, original) = (rows(-4, 8, RowMean::Permittivity, core), rows(-4, 8, RowMean::Index, core));
		assert_eq!((engine[1], engine[6]), (1.58, 1.58), "half of index 2: sqrt((1 + 1 + 4 + 4) / 4)");
		assert_eq!((original[1], original[6]), (1.5, 1.5));
		assert_eq!(&engine[2..6], &[2.0; 4]);
	}

	#[test]
	fn harness_fixture() {
		let s = fourth();
		let om = s.omega_term(16.0);
		assert!((om - 0.1539924953).abs() < 1e-10);
		let profile = rows(-57, 114, s.rows, |y| {
			if y.abs() <= 4.0 {
				1.6
			} else if y.abs() <= 53.0 {
				1.45
			} else {
				1.0
			}
		});
		let beta = s.beta(&profile, om, false).unwrap().beta;
		assert!((beta - 0.5985080080).abs() < 1e-9, "{beta}");
	}

	#[test]
	fn nine_point_guides_and_tends_to_the_continuum() {
		let s = ModeSolver::new(Stencil::NinePoint, 2);
		let lam = 32.0;
		let om = s.omega_term(lam);
		let mode = s.single(lam, om, 1.6, 1.45, 10, 0).unwrap();
		let k0 = std::f64::consts::TAU / lam;
		let n_eff = mode.beta / k0;
		assert!(n_eff > 1.45 && n_eff < 1.6, "{n_eff}");
		// the same guide at 4th order: both stencils approximate one Laplacian at 32 cells a wavelength
		let f = ModeSolver::new(Stencil::FourthOrder, 2);
		let other = f.single(lam, f.omega_term(lam), 1.6, 1.45, 10, 0).unwrap();
		assert!((mode.beta - other.beta).abs() < 0.01 * other.beta, "{} vs {}", mode.beta, other.beta);
		let kappa = s.kappa_at(16.0, om, 1.6, 1.45, 10, 20);
		assert!(kappa > 0.0);
		assert!(s.beta(&[1.0; 40], om, false).is_none());
	}
}
