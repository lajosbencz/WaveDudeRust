use std::f64::consts::PI;

use anyhow::{Result, bail};
use clap::Args as ClapArgs;
use wavedude_core::engine::stencil::{StencilCoefficients, TimeScheme};
use wavedude_core::engine::substeps::sub_steps;
use wavedude_core::model::Stencil;

#[derive(ClapArgs)]
pub(crate) struct Args {
	/// Cells per wavelength to tabulate.
	#[arg(long, value_delimiter = ',', default_values_t = [4.0, 5.0, 6.0, 8.0, 10.0, 16.0, 20.0, 32.0])]
	cells: Vec<f64>,
}

const STENCILS: [(Stencil, &str); 2] = [(Stencil::NinePoint, "9-point"), (Stencil::FourthOrder, "4th order (DRP)")];

/// Phase velocity over c (speed 1) of a plane wave of `cells` per wavelength at `angle` to the x axis, after
/// `n` sub-steps of `1/n` per step of the engine's stencil and time scheme (`wavedude_core::engine::stencil`).
fn phase_velocity(stencil: Stencil, cells: f64, angle: f64, n: u32) -> f64 {
	let k = 2.0 * PI / cells;
	let symbol = StencilCoefficients::of(stencil).symbol(k * angle.cos(), k * angle.sin());
	n as f64 * TimeScheme::of(stencil).phase(symbol / (n * n) as f64) / k
}

pub(crate) fn run(args: &Args) -> Result<()> {
	if args.cells.iter().any(|&c| !c.is_finite() || c < 2.0) {
		bail!("cells per wavelength must be at least 2 (the grid's Nyquist limit)");
	}
	println!("Phase velocity v/c at c = 1 (index 1), along an axis and on the diagonal (45 degrees).");
	println!();
	let mut header = String::from("| cells/wavelength |");
	let mut rule = String::from("|---|");
	for (_, name) in STENCILS {
		for direction in ["axis", "diagonal"] {
			header += &format!(" {name} {direction} |");
			rule += "---|";
		}
	}
	println!("{header}");
	println!("{rule}");
	for &cells in &args.cells {
		let mut row = format!("| {cells} |");
		for (stencil, _) in STENCILS {
			let n = sub_steps(stencil, 1.0);
			for angle in [0.0, PI / 4.0] {
				let v = phase_velocity(stencil, cells, angle, n);
				row += &format!(" {v:.5} ({:+.3}%) |", (v - 1.0) * 100.0);
			}
		}
		println!("{row}");
	}
	println!();
	for (stencil, name) in STENCILS {
		println!("{name}: {} sub-steps per step at index 1", sub_steps(stencil, 1.0));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn long_waves_travel_at_c() {
		for (stencil, _) in STENCILS {
			let v = phase_velocity(stencil, 1000.0, 0.3, sub_steps(stencil, 1.0));
			assert!((v - 1.0).abs() < 1e-5, "{stencil:?}: {v}");
		}
	}

	#[test]
	fn the_nine_point_stencil_is_isotropic_to_leading_order() {
		let n = sub_steps(Stencil::NinePoint, 1.0);
		let (axis, diagonal) =
			(phase_velocity(Stencil::NinePoint, 20.0, 0.0, n), phase_velocity(Stencil::NinePoint, 20.0, PI / 4.0, n));
		assert!((axis - diagonal).abs() < 2e-4);
	}
}
