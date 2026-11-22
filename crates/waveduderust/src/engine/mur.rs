//! Mur's first-order absorbing condition on open edges without a rim, shared by both engines (docs/physics.md,
//! Open edges).

use super::stencil::PAD;
use super::types::SceneData;
use crate::model::Edges;

/// One column or row of Mur's condition: a grid cell on the edge and the three padding cells
/// outward of it, with the weights of the first ring's update (`weights`, see [`mur_weights`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MurChain {
	pub inner: usize,
	pub rings: [usize; PAD],
	pub weights: [f32; 3],
}

/// Weights `[a, b, c]` of Mur's first-order condition at an edge cell of squared Courant number `c2` and damping `d`
/// per sub-step: `ring+ = a inner + b inner+ + c ring`.
///
/// The centred update solves `u_tt + gamma u_t = c^2 Lap u` with `gamma dt = 2g`, `g = (1 - d) / (1 + d)`, which
/// factors as `(D_t + gamma/2 - c D_n)(D_t + gamma/2 + c D_n)` up to `gamma^2 / 4`. Its outgoing factor
/// `u_t + gamma/2 u - c u_n = 0` (n the inward normal), centred between the two cells and the two time levels, gives
/// with `S = sqrt(c2)` (cells a sub-step)
///
/// ```text
/// (1 + S + g/2) ring+ = (1 + S - g/2) inner - (1 - S + g/2) inner+ + (1 - S - g/2) ring
/// ```
///
/// At d = 1 this is Mur's `ring+ = inner + k (inner+ - ring)`, `k = (S - 1)/(S + 1)`. S is the long-wave phase speed
/// of both schemes, so a normally incident long wave is absorbed exactly whatever the stencil.
pub(crate) fn mur_weights(c2: f32, d: f32) -> [f32; 3] {
	let (s, g) = (c2.sqrt() as f64, (1.0 - d as f64) / (1.0 + d as f64));
	let den = 1.0 + s + g / 2.0;
	[(1.0 + s - g / 2.0) / den, -(1.0 - s + g / 2.0) / den, (1.0 - s - g / 2.0) / den].map(|w| w as f32)
}

/// The Mur chains of a padded grid, none unless its edges are open without a rim (a rim is a PML backed
/// by the wall): every padding cell once, the top and bottom rows over the full padded width (the corners
/// from the nearest grid cell), the left and right columns over the grid's rows. `c2` and `d` per padded
/// cell at the sub-step scale.
pub(crate) fn mur_chains(scene: &SceneData, c2: &[f32], d: &[f32]) -> Vec<MurChain> {
	if scene.edges != Edges::Open || scene.rim != 0 {
		return Vec::new();
	}
	let (gw, gh, s) = (scene.grid_width() as usize, scene.grid_height() as usize, scene.stride());
	let at = |x: usize, y: usize| y * s + x;
	let chain =
		|inner: usize, rings: [usize; PAD]| MurChain { inner, rings, weights: mur_weights(c2[inner], d[inner]) };
	let mut chains = Vec::with_capacity(2 * (s + gh));
	for x in 0..s {
		let ix = x.clamp(PAD, PAD + gw - 1);
		chains.push(chain(at(ix, PAD), [1, 2, 3].map(|r| at(x, PAD - r))));
		chains.push(chain(at(ix, PAD + gh - 1), [1, 2, 3].map(|r| at(x, PAD + gh - 1 + r))));
	}
	for y in PAD..PAD + gh {
		chains.push(chain(at(PAD, y), [1, 2, 3].map(|r| at(PAD - r, y))));
		chains.push(chain(at(PAD + gw - 1, y), [1, 2, 3].map(|r| at(PAD + gw - 1 + r, y))));
	}
	chains
}
