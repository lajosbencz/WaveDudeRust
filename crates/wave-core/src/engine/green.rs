//! The lattice Green's function of the engines' discrete operators at the source cell, which
//! normalises a source's forcing (docs/physics.md, Sources).
//!
//! A forcing `f0 e^{-i omega t}` at one cell of an infinite grid of speed c answers in the steady
//! state with `f0 G0(omega)` at that cell. Per sub-step of `dt = 1/n` a mode of symbol sigma
//! (engine/stencil.rs) with `x = dt^2 c^2 sigma` and `W = 4 sin^2(omega dt / 2)` obeys
//!
//! ```text
//! leapfrog:           U = F / (dt^2 (c^2 sigma - n^2 W))         F = dt^2 f0
//! modified equation:  U = F (1 - x/12) / (x - x^2/12 - W)        F = dt^2 f0 (1 - (omega dt)^2 / 12)
//! ```
//!
//! (engine/forcing.rs gives F). The second splits into `sum_i B_i / (x - x_i)` over the roots
//! `x_i = 6 -+ sqrt(36 - 12 W)`, so both reduce to the resolvent at the source cell,
//! `R(z) = (2 pi)^-2 int over [-pi, pi]^2 of dk / (c^2 sigma(k) - z)`, taken outgoing: W + i eta.
//!
//! R is computed as an iterated integral. For fixed ky, `c^2 sigma - z` is a polynomial in `cos kx`
//! of degree M (the stencil's reach), so the kx integral is exact by partial fractions (`inner`).
//! The ky integral, over [0, pi] by symmetry, is split where the kx band
//! `[sigma(0, ky), sigma(pi, ky)]` (both monotone in ky) has an edge at Re z, where the integrand
//! has an inverse square root singularity; each piece is mapped by `ky = a + (b - a)(3u^2 - 2u^3)`,
//! which removes it, cut into pieces halving towards both ends (the integrand varies on the scale of
//! the wave number near ky = 0 and near a singular point) and integrated by Gauss-Legendre. The
//! error budget: eta = 1e-9 (relative to max(1, |W|)) and the quadrature; against halving eta and
//! doubling the nodes |G0| moves by less than 1e-5 (tests below), against a direct sum over the
//! Brillouin zone at a resolvable shift R agrees to 1e-9.
//!
//! At a lattice offset (m, l) from the forced cell the response carries `e^{i(m kx + l ky)}`: the kx
//! integral gains the factor `(r + xi)^m` per root and the ky integrand `cos(l ky)`. A source spread
//! over the 2 x 2 cells around a fractional position with bilinear shares w_i answers at that
//! position (the field sampled with the same shares) with `sum_ij w_i w_j G(c_i - c_j)`, which
//! `spread_response` gives.

use std::f64::consts::PI;

use num_complex::Complex64 as C;

use super::stencil::{StencilCoefficients, TimeScheme};

/// Halvings of each half of a ky interval towards its end.
const GRADING: i32 = 30;

/// The error budget of a response.
#[derive(Clone, Copy)]
struct Quadrature {
	/// Outgoing shift of W, relative to max(1, |W|).
	eta: f64,
	/// Gauss-Legendre nodes per piece.
	nodes: usize,
}

const BUDGET: Quadrature = Quadrature { eta: 1e-9, nodes: 16 };

/// `|G0|` at the source cell for a forcing of angular frequency `omega` (rad/step) at `n` sub-steps
/// on the stencil `coefficients` with `scheme`, in a medium of squared speed `c2` (1 for free space).
pub fn source_response(coefficients: &StencilCoefficients, scheme: TimeScheme, n: u32, omega: f64, c2: f64) -> f64 {
	response(coefficients, scheme, n, omega, c2, (0, 0), BUDGET).norm()
}

/// `|G|` at a source spread over the 2 x 2 cells around a position with the fractional parts
/// `(fx, fy)` in [0, 1), the field sampled there with the same bilinear shares; `source_response` at (0, 0).
pub fn spread_response(
	coefficients: &StencilCoefficients,
	scheme: TimeScheme,
	n: u32,
	omega: f64,
	c2: f64,
	(fx, fy): (f64, f64),
) -> f64 {
	let g = |offset| response(coefficients, scheme, n, omega, c2, offset, BUDGET);
	let [w00, w10, w01, w11] = bilinear(fx, fy);
	let mut total = g((0, 0)) * (w00 * w00 + w10 * w10 + w01 * w01 + w11 * w11);
	if fx > 0.0 {
		total += g((1, 0)) * (2.0 * (w00 * w10 + w01 * w11));
	}
	if fy > 0.0 {
		total += g((0, 1)) * (2.0 * (w00 * w01 + w10 * w11));
	}
	if fx > 0.0 && fy > 0.0 {
		total += g((1, 1)) * (2.0 * (w00 * w11 + w10 * w01));
	}
	total.norm()
}

/// Bilinear shares of the cells at (0, 0), (1, 0), (0, 1) and (1, 1) around the fractional position `(fx, fy)`.
pub fn bilinear(fx: f64, fy: f64) -> [f64; 4] {
	[(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy]
}

fn response(
	coefficients: &StencilCoefficients,
	scheme: TimeScheme,
	n: u32,
	omega: f64,
	c2: f64,
	offset: (u32, u32),
	budget: Quadrature,
) -> C {
	let (nf, theta) = (n as f64, omega / n as f64);
	let w = 4.0 * (theta / 2.0).sin().powi(2);
	let w = C::new(w, budget.eta * w.abs().max(1.0));
	let r = |z: C| resolvent(coefficients, c2, z, offset, budget.nodes);
	match scheme {
		TimeScheme::Leapfrog => r(w * nf * nf),
		TimeScheme::ModifiedEquation => {
			let root = (C::from(36.0) - w * 12.0).sqrt();
			let (x1, x2) = (C::from(6.0) - root, C::from(6.0) + root);
			let numerator = |x: C| C::from(1.0) - x / 12.0;
			let (b1, b2) = (numerator(x1) * -12.0 / (x1 - x2), numerator(x2) * -12.0 / (x2 - x1));
			(b1 * r(x1 * nf * nf) + b2 * r(x2 * nf * nf)) * (1.0 - theta * theta / 12.0)
		}
	}
}

/// `R(z) = (2 pi)^-2 int e^{i(m kx + l ky)} dk / (c2 sigma(k) - z)` for z off the real axis at the lattice
/// offset `(m, l)`.
fn resolvent(coefficients: &StencilCoefficients, c2: f64, z: C, (m, l): (u32, u32), nodes: usize) -> C {
	let edge = |kx: f64| move |ky: f64| c2 * coefficients.symbol(kx, ky) - z.re;
	let mut cuts = vec![0.0, PI];
	for f in [edge(0.0), edge(PI)] {
		if let Some(k) = crossing(f) {
			cuts.push(k);
		}
	}
	cuts.sort_by(f64::total_cmp);
	let (u, wu) = gauss_legendre(nodes);
	let mut sum = C::from(0.0);
	for pair in cuts.windows(2) {
		for (a, b) in graded(pair[0], pair[1]) {
			let span = b - a;
			for (&ui, &wi) in u.iter().zip(&wu) {
				let ky = a + span * (3.0 * ui * ui - 2.0 * ui * ui * ui);
				let jacobian = span * 6.0 * ui * (1.0 - ui);
				sum += inner(coefficients, c2, z, ky, m) * (wi * jacobian * (l as f64 * ky).cos());
			}
		}
	}
	sum / PI
}

/// Pieces of [a, b] halving towards both ends, [`GRADING`] times on each side.
fn graded(a: f64, b: f64) -> Vec<(f64, f64)> {
	let mid = 0.5 * (a + b);
	let mut pieces = vec![(a, a + (mid - a) * 0.5f64.powi(GRADING)), (b - (b - mid) * 0.5f64.powi(GRADING), b)];
	for j in 0..GRADING {
		let (near, far) = (0.5f64.powi(j + 1), 0.5f64.powi(j));
		pieces.push((a + (mid - a) * near, a + (mid - a) * far));
		pieces.push((b - (b - mid) * far, b - (b - mid) * near));
	}
	pieces
}

/// The kx integral `(2 pi)^-1 int e^{i m kx} dkx / (c2 sigma(kx, ky) - z)`: `c2 sigma - z = Q(cos kx)`
/// with Q of the stencil's reach in degree, so by partial fractions over the roots xi of Q it is
/// `sum (r + xi)^m / (Q'(xi) r(xi))`, `(2 pi)^-1 int e^{i m k} dk / (cos k - xi) = (r + xi)^m / r`,
/// `r = -+sqrt(xi^2 - 1)` with `|r + xi| < 1`.
fn inner(coefficients: &StencilCoefficients, c2: f64, z: C, ky: f64, m: u32) -> C {
	let [a0, a1, a2, a3] = coefficients.axis;
	let q = 2.0 - 2.0 * ky.cos();
	let cross = coefficients.cross * q;
	let mut poly = vec![
		C::from(c2 * (-a0 + 2.0 * a2 + coefficients.axis_symbol(ky) - 2.0 * cross)) - z,
		C::from(c2 * (-2.0 * a1 + 6.0 * a3 + 2.0 * cross)),
		C::from(c2 * -4.0 * a2),
		C::from(c2 * -8.0 * a3),
	];
	poly.truncate(coefficients.reach() + 1);
	let derivative =
		|x: C| poly.iter().enumerate().skip(1).rev().fold(C::from(0.0), |acc, (k, &c)| acc * x + c * k as f64);
	poly_roots(&poly)
		.into_iter()
		.map(|xi| {
			let mut r = (xi * xi - 1.0).sqrt();
			if (r + xi).norm() >= 1.0 {
				r = -r;
			}
			(r + xi).powu(m) / (derivative(xi) * r)
		})
		.sum()
}

/// The root of a monotone `f` on [0, pi], if it changes sign there.
fn crossing(f: impl Fn(f64) -> f64) -> Option<f64> {
	let (mut lo, mut hi) = (0.0, PI);
	let (flo, fhi) = (f(lo), f(hi));
	if flo.signum() == fhi.signum() || flo == 0.0 || fhi == 0.0 {
		return None;
	}
	for _ in 0..200 {
		let mid = 0.5 * (lo + hi);
		if f(mid).signum() == flo.signum() {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	Some(0.5 * (lo + hi))
}

/// Roots of `sum c_k t^k` (c in ascending order, leading coefficient non-zero) by Durand-Kerner,
/// polished by Newton.
fn poly_roots(c: &[C]) -> Vec<C> {
	let degree = c.len() - 1;
	let lead = c[degree];
	let monic: Vec<C> = c.iter().map(|&v| v / lead).collect();
	let eval = |t: C| monic.iter().rev().fold(C::from(0.0), |acc, &ck| acc * t + ck);
	let slope =
		|t: C| monic.iter().enumerate().skip(1).rev().fold(C::from(0.0), |acc, (k, &ck)| acc * t + ck * k as f64);
	if degree == 1 {
		return vec![-monic[0]];
	}
	let seed = C::new(0.4, 0.9);
	let mut roots: Vec<C> = (0..degree).map(|k| seed.powu(k as u32)).collect();
	for _ in 0..500 {
		let mut change = 0f64;
		for i in 0..degree {
			let denominator: C =
				(0..degree).filter(|&j| j != i).map(|j| roots[i] - roots[j]).fold(C::from(1.0), |acc, d| acc * d);
			let step = eval(roots[i]) / denominator;
			roots[i] -= step;
			change = change.max(step.norm());
		}
		if change < 1e-15 {
			break;
		}
	}
	for r in &mut roots {
		for _ in 0..3 {
			let d = slope(*r);
			if d.norm() > 0.0 {
				*r -= eval(*r) / d;
			}
		}
	}
	roots
}

/// Gauss-Legendre nodes and weights on [0, 1].
fn gauss_legendre(n: usize) -> (Vec<f64>, Vec<f64>) {
	let (mut x, mut w) = (Vec::with_capacity(n), Vec::with_capacity(n));
	for i in 0..n {
		let mut z = (PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
		let mut dp = 0.0;
		for _ in 0..100 {
			let (mut p0, mut p1) = (1.0, z);
			for k in 2..=n {
				let p2 = ((2 * k - 1) as f64 * z * p1 - (k - 1) as f64 * p0) / k as f64;
				p0 = p1;
				p1 = p2;
			}
			dp = n as f64 * (z * p1 - p0) / (z * z - 1.0);
			let dz = p1 / dp;
			z -= dz;
			if dz.abs() < 1e-16 {
				break;
			}
		}
		x.push(0.5 * (1.0 - z));
		w.push(1.0 / ((1.0 - z * z) * dp * dp));
	}
	(x, w)
}

#[cfg(test)]
mod tests {
	use super::*;

	const SETS: [StencilCoefficients; 2] = [StencilCoefficients::NINE_POINT, StencilCoefficients::DRP];

	#[test]
	fn quadrature_integrates_polynomials() {
		let (x, w) = gauss_legendre(8);
		assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-14);
		let fifth: f64 = x.iter().zip(&w).map(|(x, w)| w * x.powi(5)).sum();
		assert!((fifth - 1.0 / 6.0).abs() < 1e-14);
	}

	/// Against a direct sum over a 2048^2 grid of the Brillouin zone at a shift the grid resolves, at the
	/// origin and at the offsets a spread source needs.
	#[test]
	fn resolvent_matches_a_direct_sum() {
		for set in SETS {
			for z in [C::new(0.3, 0.2), C::new(2.0, 0.3)] {
				for offset in [(0u32, 0u32), (1, 0), (0, 1), (1, 1)] {
					let n = 2048;
					let h = 2.0 * PI / n as f64;
					let mut direct = C::from(0.0);
					for i in 0..n {
						for j in 0..n {
							let (kx, ky) = (-PI + (i as f64 + 0.5) * h, -PI + (j as f64 + 0.5) * h);
							let phase = (offset.0 as f64 * kx + offset.1 as f64 * ky).cos();
							direct += phase / (C::from(set.symbol(kx, ky)) - z);
						}
					}
					direct /= (n * n) as f64;
					let ours = resolvent(&set, 1.0, z, offset, BUDGET.nodes);
					assert!(
						(ours - direct).norm() < 1e-9 * direct.norm(),
						"{set:?} {z} {offset:?}: {ours} against {direct}"
					);
				}
			}
		}
	}

	/// A whole-number position is the plain cell response; a spread source answers less than one cell
	/// does, since its neighbours' responses are weaker and lag.
	#[test]
	fn spread_response_reduces_to_the_cell_and_falls_off_axis() {
		for set in SETS {
			let g0 = source_response(&set, TimeScheme::ModifiedEquation, 2, 0.4, 1.0);
			assert_eq!(spread_response(&set, TimeScheme::ModifiedEquation, 2, 0.4, 1.0, (0.0, 0.0)), g0);
			let half = spread_response(&set, TimeScheme::ModifiedEquation, 2, 0.4, 1.0, (0.5, 0.0));
			let corner = spread_response(&set, TimeScheme::ModifiedEquation, 2, 0.4, 1.0, (0.5, 0.5));
			assert!(corner < half && half < g0, "{set:?}: {g0} {half} {corner}");
			assert!(half > 0.5 * g0, "{set:?}: a half-cell offset keeps most of the response, {half} of {g0}");
		}
	}

	/// The outgoing limit: Im R -> 1/4 at low frequency on every consistent Laplacian (the continuum
	/// 2D Green's function's i/4), and the result holds against halving eta and doubling the nodes.
	#[test]
	fn response_is_converged_and_outgoing() {
		for set in SETS {
			let small = resolvent(&set, 1.0, C::new(1e-4, 1e-13), (0, 0), BUDGET.nodes);
			assert!((small.im - 0.25).abs() < 1e-3, "{set:?}: {small}");
			for scheme in [TimeScheme::Leapfrog, TimeScheme::ModifiedEquation] {
				for n in [1, 2, 3] {
					for omega in [0.02, 0.1, 0.4, 1.0] {
						let base = response(&set, scheme, n, omega, 1.0, (0, 0), BUDGET);
						let finer = Quadrature { eta: BUDGET.eta / 2.0, nodes: 2 * BUDGET.nodes };
						let finer = response(&set, scheme, n, omega, 1.0, (0, 0), finer);
						let off = (base.norm() - finer.norm()).abs() / finer.norm();
						assert!(off < 1e-5, "{set:?} {scheme:?} n {n} omega {omega}: {base} against {finer}");
					}
				}
			}
		}
	}
}
