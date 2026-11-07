//! Sub-steps per simulation step from the CFL bound of the stencil and time scheme (docs/physics.md,
//! Stability).
//!
//! A scheme is stable while `s = S^2 symbol` stays under its limit for every mode, S = c dt/dx the
//! Courant number: `s <= 4` for leapfrog, `s < 12` for the modified-equation scheme (see
//! [`TimeScheme::stability_limit`]). With the symbol's largest value `M` (16/3 for the 9-point, 12.632
//! for the DRP stencil, both at (pi, pi)) the Courant limit is `sqrt(limit / M)`:
//!
//! ```text
//! 9-point, leapfrog:            sqrt(4 / (16/3)) = sqrt(3/4) = 0.8660
//! DRP 7-point, modified eq.:    sqrt(12 / 12.632)            = 0.9747
//! ```
//!
//! (leapfrog with the DRP stencil would allow 0.5627). A step is split into `n` equal sub-steps of
//! `dt = 1/n`:
//!
//! ```text
//! n = max(1, ceil(sqrt(max_c2) / (limit * 0.95)))
//! ```
//!
//! the 0.95 keeping a 5 percent margin under the bound. Per sub-step the engine scales c2 by
//! `1/n^2` and takes the damping factor as `d^(1/n)`, so a whole step loses what one step of `d` does.

use super::stencil::{StencilCoefficients, TimeScheme};
use crate::model::Stencil;

/// Share of the CFL limit a sub-step may use.
pub const CFL_SAFETY: f64 = 0.95;

/// Largest stable Courant number `c dt/dx` of the engine's scheme for `stencil`.
pub fn courant_limit(stencil: Stencil) -> f64 {
	(TimeScheme::of(stencil).stability_limit() / StencilCoefficients::of(stencil).max_symbol()).sqrt()
}

/// Sub-steps per step for a scene whose largest squared speed is `max_c2` (cells^2/step^2); 1 for an all-mirror scene.
pub fn sub_steps(stencil: Stencil, max_c2: f32) -> u32 {
	if max_c2 <= 0.0 {
		return 1;
	}
	let n = ((max_c2 as f64).sqrt() / (courant_limit(stencil) * CFL_SAFETY)).ceil();
	(n as u32).max(1)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn limits() {
		assert!((courant_limit(Stencil::NinePoint) - 0.75f64.sqrt()).abs() < 1e-12);
		assert!(
			(courant_limit(Stencil::FourthOrder) - 0.97468).abs() < 1e-5,
			"{}",
			courant_limit(Stencil::FourthOrder)
		);
	}

	#[test]
	fn unit_speed_needs_two_on_both_stencils() {
		assert_eq!(sub_steps(Stencil::NinePoint, 1.0), 2);
		assert_eq!(sub_steps(Stencil::FourthOrder, 1.0), 2);
		assert_eq!(sub_steps(Stencil::FourthOrder, 0.8), 1);
	}

	#[test]
	fn slow_media_and_empty_scenes_take_one() {
		assert_eq!(sub_steps(Stencil::FourthOrder, 0.25), 1);
		assert_eq!(sub_steps(Stencil::NinePoint, 0.0), 1);
	}

	#[test]
	fn courant_number_stays_under_the_bound_with_margin() {
		for stencil in [Stencil::NinePoint, Stencil::FourthOrder] {
			for i in 1..=1000 {
				let c2 = i as f32 / 1000.0 * 4.0;
				let n = sub_steps(stencil, c2);
				let s = (c2 as f64).sqrt() / n as f64;
				assert!(s <= courant_limit(stencil) * CFL_SAFETY + 1e-12, "{stencil:?} c2 {c2}: S {s}");
				if n > 1 {
					let fewer = (c2 as f64).sqrt() / (n - 1) as f64;
					assert!(fewer > courant_limit(stencil) * CFL_SAFETY, "{stencil:?} c2 {c2}: n not minimal");
				}
			}
		}
	}
}
