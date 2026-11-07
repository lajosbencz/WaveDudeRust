//! Stencil coefficients and time schemes (docs/physics.md, Spatial stencils and Time stepping).
//!
//! A Laplacian here is `sum over axes of D(u) + cross D3x(D3y(u))`, where `D` is a symmetric
//! second-derivative stencil of up to 7 points along one axis and `D3` the 3-point one. Its symbol
//! (the positive `-L(e^{ik.x}) / e^{ik.x}`) is `p(kx) + p(ky) - cross q(kx) q(ky)` with
//! `p(k) = -(a0 + 2 sum a_m cos(m k))` and `q(k) = 2 - 2 cos k`.

use crate::model::Stencil;

/// Padding rings around the grid: the widest stencil's reach.
pub const PAD: usize = 3;

/// The DRP second-derivative weights a0..a3 (cells^-2): the 7-point symmetric stencil exact to 4th
/// order (`sum a = 0`, `sum a_m m^2 = 1`, `sum a_m m^4 = 0` over both sides) whose one free weight
/// minimises `int_0^{pi/2} ((p(k) - k^2) / 2k^2)^2 dk`, the squared relative phase error (Tam and
/// Webb 1993, least squares). Closed form in a3: `a1 = 4/3 + 15 a3`, `a2 = -1/12 - 6 a3`,
/// `a0 = -5/2 - 20 a3`. Phase error along an axis: -0.23 % at 4 cells a wavelength, +0.094 % at 6,
/// +0.025 % at 10.
pub const DRP_WEIGHTS: [f64; 4] = [-2.8070216503722194, 1.5635995711124977, -0.17543982844499903, 0.015351082518610951];

/// The Taylor 4th-order weights (-1/12, 4/3, -5/2): the original's 4th-order stencil.
pub const TAYLOR_FOURTH_WEIGHTS: [f64; 4] = [-2.5, 4.0 / 3.0, -1.0 / 12.0, 0.0];

/// The 3-point second difference.
pub const SECOND_DIFFERENCE_WEIGHTS: [f64; 4] = [-2.0, 1.0, 0.0, 0.0];

/// A Laplacian's weights (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StencilCoefficients {
	/// Second-derivative weights along one axis at offsets 0..=3, the same on both sides.
	pub axis: [f64; 4],
	/// Weight of the mixed term `D3x D3y`.
	pub cross: f64,
}

impl StencilCoefficients {
	/// The isotropic 9-point Laplacian: `D3x + D3y + D3x D3y / 6` (Patra and Karttunen 2006).
	pub const NINE_POINT: Self = Self { axis: SECOND_DIFFERENCE_WEIGHTS, cross: 1.0 / 6.0 };
	/// The 7-point-per-axis DRP Laplacian.
	pub const DRP: Self = Self { axis: DRP_WEIGHTS, cross: 0.0 };
	/// The Taylor 4th-order 5-point-per-axis Laplacian.
	pub const TAYLOR_FOURTH: Self = Self { axis: TAYLOR_FOURTH_WEIGHTS, cross: 0.0 };

	/// The engine's coefficients for `stencil`.
	pub fn of(stencil: Stencil) -> Self {
		match stencil {
			Stencil::NinePoint => Self::NINE_POINT,
			Stencil::FourthOrder => Self::DRP,
		}
	}

	/// Cells the stencil reaches along an axis.
	pub fn reach(&self) -> usize {
		(1..4).rev().find(|&m| self.axis[m] != 0.0).unwrap_or(1)
	}

	/// `p(k)`: the axis symbol, k^2 + O(k^6) for the 4th-order sets.
	pub fn axis_symbol(&self, k: f64) -> f64 {
		-(self.axis[0] + 2.0 * (1..4).map(|m| self.axis[m] * (m as f64 * k).cos()).sum::<f64>())
	}

	/// The Laplacian's symbol for wave numbers (kx, ky) in radians per cell.
	pub fn symbol(&self, kx: f64, ky: f64) -> f64 {
		let q = |k: f64| 2.0 - 2.0 * k.cos();
		self.axis_symbol(kx) + self.axis_symbol(ky) - self.cross * q(kx) * q(ky)
	}

	/// The symbol's largest value, at (pi, pi) for every set here (the axis symbols rise on [0, pi]
	/// and the cross weight is below 1/4).
	pub fn max_symbol(&self) -> f64 {
		self.symbol(std::f64::consts::PI, std::f64::consts::PI)
	}

	/// The inverse of [`axis_symbol`](Self::axis_symbol) on [0, pi] by bisection.
	pub fn axis_wavenumber(&self, symbol: f64) -> f64 {
		let (mut lo, mut hi) = (0.0, std::f64::consts::PI);
		for _ in 0..100 {
			let mid = 0.5 * (lo + hi);
			if self.axis_symbol(mid) < symbol {
				lo = mid;
			} else {
				hi = mid;
			}
		}
		0.5 * (lo + hi)
	}
}

/// How a sub-step advances the field. With `A = dt^2 c^2 L` (L the Laplacian, dt the sub-step):
///
/// ```text
/// Leapfrog:           u+ = 2u - u- + A u
/// ModifiedEquation:   u+ = 2u - u- + A u + A (A u) / 12
/// ```
///
/// The second (Dablain 1986) cancels leapfrog's leading temporal error `dt^4 u_tttt / 12` by
/// `u_tttt = (c^2 L)^2 u`, so it is 4th order in time. For a mode of A with eigenvalue `-s` a
/// sub-step turns `e^{-i theta}` into `4 sin^2(theta/2) = s` (leapfrog) or `s - s^2/12`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeScheme {
	Leapfrog,
	ModifiedEquation,
}

impl TimeScheme {
	/// The engine's scheme for `stencil`: the 4th-order stencil runs 4th order in time.
	pub fn of(stencil: Stencil) -> Self {
		match stencil {
			Stencil::NinePoint => Self::Leapfrog,
			Stencil::FourthOrder => Self::ModifiedEquation,
		}
	}

	/// Largest stable `s = S^2 symbol` (S the Courant number): `4 sin^2(theta/2)` must lie in [0, 4]
	/// for a real theta. Leapfrog: s <= 4. Modified equation: `s - s^2/12` is at most 3 and is
	/// non-negative up to s = 12, where the two roots of the amplification factor meet; so s < 12,
	/// sqrt(3) times leapfrog's Courant limit.
	pub fn stability_limit(self) -> f64 {
		match self {
			Self::Leapfrog => 4.0,
			Self::ModifiedEquation => 12.0,
		}
	}

	/// Phase per sub-step of a mode with `s = S^2 symbol`.
	pub fn phase(self, s: f64) -> f64 {
		let rhs = match self {
			Self::Leapfrog => s,
			Self::ModifiedEquation => s - s * s / 12.0,
		};
		2.0 * (rhs.sqrt() / 2.0).asin()
	}

	/// The `s` of a mode whose phase per sub-step is `theta` (the inverse of [`phase`](Self::phase)
	/// on its rising branch).
	pub fn symbol_of_phase(self, theta: f64) -> f64 {
		let om = 4.0 * (theta / 2.0).sin().powi(2);
		match self {
			Self::Leapfrog => om,
			Self::ModifiedEquation => 6.0 * (1.0 - (1.0 - om / 3.0).max(0.0).sqrt()),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::f64::consts::PI;

	#[test]
	fn drp_weights_are_consistent_to_fourth_order() {
		let a = DRP_WEIGHTS;
		assert!((a[0] + 2.0 * (a[1] + a[2] + a[3])).abs() < 1e-15);
		assert!((a[1] + 4.0 * a[2] + 9.0 * a[3] - 1.0).abs() < 1e-15);
		assert!((a[1] + 16.0 * a[2] + 81.0 * a[3]).abs() < 1e-14);
		assert!((a[1] - (4.0 / 3.0 + 15.0 * a[3])).abs() < 1e-15 && (a[2] - (-1.0 / 12.0 - 6.0 * a[3])).abs() < 1e-15);
	}

	#[test]
	fn drp_weights_minimise_the_phase_error() {
		let error = |a3: f64| {
			let c = StencilCoefficients {
				axis: [-2.5 - 20.0 * a3, 4.0 / 3.0 + 15.0 * a3, -1.0 / 12.0 - 6.0 * a3, a3],
				cross: 0.0,
			};
			let n = 4000;
			(0..n)
				.map(|i| {
					let k = (i as f64 + 0.5) * PI / 2.0 / n as f64;
					((c.axis_symbol(k) - k * k) / (2.0 * k * k)).powi(2)
				})
				.sum::<f64>()
		};
		let best = DRP_WEIGHTS[3];
		assert!(error(best) < error(best * 1.001) && error(best) < error(best * 0.999));
	}

	#[test]
	fn symbols_peak_at_the_corner() {
		for c in [StencilCoefficients::NINE_POINT, StencilCoefficients::DRP, StencilCoefficients::TAYLOR_FOURTH] {
			let n = 128;
			let top = (0..=n)
				.flat_map(|i| (0..=n).map(move |j| (i, j)))
				.map(|(i, j)| c.symbol(i as f64 * PI / n as f64, j as f64 * PI / n as f64))
				.fold(0.0, f64::max);
			assert!((top - c.max_symbol()).abs() < 1e-12, "{c:?}: {top}");
		}
		assert!((StencilCoefficients::NINE_POINT.max_symbol() - 16.0 / 3.0).abs() < 1e-12);
		assert!((StencilCoefficients::TAYLOR_FOURTH.max_symbol() - 32.0 / 3.0).abs() < 1e-12);
	}

	#[test]
	fn nine_point_symbol_is_the_original_form() {
		let c = StencilCoefficients::NINE_POINT;
		for (kx, ky) in [(0.3f64, 1.1f64), (2.0, 0.0), (PI, 0.5)] {
			let old = 10.0 / 3.0 - 4.0 * kx.cos() * ky.cos() / 6.0 - 2.0 / 3.0 * (2.0 * kx.cos() + 2.0 * ky.cos());
			assert!((c.symbol(kx, ky) - old).abs() < 1e-14);
		}
	}

	#[test]
	fn phase_and_its_inverse() {
		for scheme in [TimeScheme::Leapfrog, TimeScheme::ModifiedEquation] {
			for s in [0.01, 0.5, 2.0, 3.5] {
				assert!((scheme.symbol_of_phase(scheme.phase(s)) - s).abs() < 1e-12, "{scheme:?} {s}");
			}
		}
		let theta = 0.3;
		let s = TimeScheme::ModifiedEquation.symbol_of_phase(theta);
		assert!((s.sqrt() - theta).abs() < 2e-5 * theta, "4th order in time: {}", s.sqrt() - theta);
	}
}
