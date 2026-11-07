//! The CPML on the rim (Roden and Gedney 2000) for `u_tt = c^2 lap u`, in the second-order form
//! with memory variables for the stretched derivatives (as Komatitsch and Martin 2007 and Pasalic and
//! McGarry 2010 do for the second-order acoustic equation).
//!
//! Along x the PML replaces `d/dx` by `d/dx~ = (1/s) d/dx`, `s = kappa + sigma / (alpha + i omega)`;
//! `1/s` is `1/kappa` plus a causal kernel whose convolution is advanced recursively per sub-step dt:
//!
//! ```text
//! b = exp(-(sigma/kappa + alpha) dt),   a = sigma (b - 1) / (sigma kappa + kappa^2 alpha)
//! psi <- b psi + a D1 f                         psi ~ the kernel's convolution with f_x
//! Q    = (1/kappa) D2 f + (1/kappa)' D1 f + D1 psi    = d/dx (f_x / kappa + psi)
//! xi  <- b xi + a Q
//! O_x f = f_x~x~ = Q / kappa + xi
//! ```
//!
//! D2 the stencil's second derivative along the axis, D1 the 4th-order central first derivative
//! `(8 (f1 - f-1) - (f2 - f-2)) / 12`, (1/kappa)' the profile's analytic slope. Inside the canvas
//! sigma = 0 and kappa = 1, so `O = D2`. The y axis is the same.
//!
//! Every Laplacian here is `D2x z + D2y u` with `z = u + chi D2y u` (chi the cross weight: 1/6 for
//! the 9-point, whose D2 is the 3-point difference; 0 for the axis stencils). Stretching both factors,
//! `lap~ u = O_x z~ + O_y u`, `z~ = u + chi O_y u`; a mixed term left unstretched makes the layer
//! unstable. With `delta_y = O_y u - D2y u` the rim's correction to the plain stencil is
//!
//! ```text
//! (O_x z~ - D2x z~) + chi D2x delta_y + delta_y
//! ```
//!
//! which the engine adds times c2 after each pass over the grid, in four stages over the rim: psi of
//! y on u, delta_y (stored) and xi of y, psi of x on z~, then the x part and the sum. A time scheme
//! applying the operator twice (the modified equation) keeps a second set of memory variables.
//!
//! Storage covers the rim only: `psi` and `xi` of x over the left and right strips (full height, 2 rim
//! columns a row), of y over the top and bottom strips (2 rim rows of full width), x first; delta_y
//! follows xi over the y strips.

use rayon::prelude::*;

use super::stencil::PAD;
use super::types::{PmlLayer, SceneData};

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
	x: Vec<[f32; 4]>,
	y: Vec<[f32; 4]>,
	axis: [f32; 4],
	cross: f32,
	psi: [Vec<f32>; 2],
	/// xi of x and y, then delta_y.
	xi: [Vec<f32>; 2],
	/// z~ of the current application per grid row, over the x strips' columns and 3 beyond each
	/// side: `2 (rim + 6)` values a row.
	z: Vec<f32>,
	zeros: Vec<f32>,
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
			x: axis_table(&scene.pml, strips.gw, dt),
			y: axis_table(&scene.pml, strips.gh, dt),
			axis,
			cross,
			psi: [vec![0.0; len], vec![0.0; len]],
			xi: [vec![0.0; xi], vec![0.0; xi]],
			z: vec![0.0; strips.gh * Self::z_width(strips.rim)],
			zeros: vec![0.0; strips.gw],
		})
	}

	pub fn reset(&mut self) {
		for b in self.psi.iter_mut().chain(&mut self.xi) {
			b.fill(0.0);
		}
	}

	/// Adds the rim's correction of `field` (times `scale c2`) to `out` and advances memory `set`.
	/// `c2` per padded cell at the sub-step scale.
	pub fn apply(&mut self, set: usize, field: &[f32], cuts: &[u8], c2: &[f32], out: &mut [f32], scale: f32) {
		self.psi_y(set, field, cuts);
		self.delta_y(set, field, cuts);
		self.psi_x(set, field, cuts);
		self.correct(set, cuts, c2, out, scale);
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

	/// Row `r + k` of `rows` (`width` values each, 2 rim of them) when it lies in the same strip
	/// (top or bottom, left or right) as `r`, else zeros.
	fn neighbour<'a>(rows: &'a [f32], zeros: &'a [f32], rim: usize, width: usize, r: usize, k: isize) -> &'a [f32] {
		let q = r as isize + k;
		let (lo, hi) = if r < rim { (0, rim as isize) } else { (rim as isize, 2 * rim as isize) };
		if q >= lo && q < hi { &rows[q as usize * width..][..width] } else { &zeros[..width] }
	}

	fn psi_y(&mut self, set: usize, field: &[f32], cuts: &[u8]) {
		let strips = self.strips;
		let (gw, s) = (strips.gw, self.stride);
		let y = &self.y;
		self.psi[set][strips.x_len()..].par_chunks_mut(gw).enumerate().for_each(|(r, line)| {
			let gy = Self::y_row(strips, r);
			let [b, a, ..] = y[gy];
			let at = (gy + PAD) * s + PAD;
			let t = column_rows(field, at, s, gw);
			let cut = &cuts[at..][..gw];
			for (x, p) in line.iter_mut().enumerate() {
				let taps = cut_taps(t.map(|row| row[x]), cut[x] >> cut::Y_SHIFT);
				*p = b * *p + a * first(&taps);
			}
		});
	}

	fn delta_y(&mut self, set: usize, field: &[f32], cuts: &[u8]) {
		let strips = self.strips;
		let (gw, s, rim, x_len, len) = (strips.gw, self.stride, strips.rim, strips.x_len(), strips.len());
		let (y, axis, psi, zeros) = (&self.y, &self.axis, &self.psi[set][x_len..], &self.zeros);
		let (xi, delta) = self.xi[set][x_len..].split_at_mut(len - x_len);
		xi.par_chunks_mut(gw).zip(delta.par_chunks_mut(gw)).enumerate().for_each(|(r, (xi, delta))| {
			let gy = Self::y_row(strips, r);
			let at = (gy + PAD) * s + PAD;
			let p = |k: isize| Self::neighbour(psi, zeros, rim, gw, r, k);
			let (p1, m1, p2, m2) = (p(1), p(-1), p(2), p(-2));
			let t = column_rows(field, at, s, gw);
			let cut = &cuts[at..][..gw];
			for x in 0..gw {
				let taps = cut_taps(t.map(|row| row[x]), cut[x] >> cut::Y_SHIFT);
				let dpsi = (8.0 * (p1[x] - m1[x]) - (p2[x] - m2[x])) / 12.0;
				delta[x] = stretched(&taps, axis, y[gy], dpsi, &mut xi[x]);
			}
		});
	}

	/// Fills `z~ = u + chi (D3y u + delta_y)` of the x strips' columns and 3 beyond, per grid row,
	/// and advances psi of x on it.
	fn psi_x(&mut self, set: usize, field: &[f32], cuts: &[u8]) {
		let strips = self.strips;
		let Strips { gw, rim, .. } = strips;
		let (s, cross, zw) = (self.stride, self.cross, Self::z_width(rim));
		let mut psi = std::mem::take(&mut self.psi[set]);
		let mut z = std::mem::take(&mut self.z);
		let (x, delta) = (&self.x, &self.xi[set][strips.len()..]);
		psi[..strips.x_len()].par_chunks_mut(2 * rim).zip(z.par_chunks_mut(zw)).enumerate().for_each(
			|(gy, (row, zrow))| {
				let base = (gy + PAD) * s + PAD;
				let delta_row = Self::strip_row(strips, gy).map(|r| &delta[r * gw..][..gw]);
				let z_at = |gx: isize| {
					let j = base.wrapping_add_signed(gx);
					let v = field[j];
					if cross == 0.0 {
						return v;
					}
					let d = match delta_row {
						Some(d) if gx >= 0 && (gx as usize) < gw => d[gx as usize],
						_ => 0.0,
					};
					v + cross * ((field[j - s] + field[j + s] - 2.0 * v) + d)
				};
				let (left, right) = zrow.split_at_mut(rim + 6);
				let right_start = (gw - rim) as isize - 3;
				for (k, (l, r)) in left.iter_mut().zip(right).enumerate() {
					*l = z_at(k as isize - 3);
					*r = z_at(right_start + k as isize);
				}
				for (col, p) in row.iter_mut().enumerate() {
					let (gx, zc) = Self::x_column(gw, rim, col);
					let taps = cut_taps(std::array::from_fn(|k| zrow[zc + k - 3]), cuts[base + gx] >> cut::X_SHIFT);
					let [b, a, ..] = x[gx];
					*p = b * *p + a * first(&taps);
				}
			},
		);
		self.psi[set] = psi;
		self.z = z;
	}

	/// The grid column of x-strip column `col` and its centre in a z row.
	fn x_column(gw: usize, rim: usize, col: usize) -> (usize, usize) {
		if col < rim { (col, col + 3) } else { (gw - 2 * rim + col, rim + 6 + (col - rim) + 3) }
	}

	fn correct(&mut self, set: usize, cuts: &[u8], c2: &[f32], out: &mut [f32], scale: f32) {
		let strips = self.strips;
		let Strips { gw, gh, rim } = strips;
		let (s, zw) = (self.stride, Self::z_width(rim));
		let mut memory = std::mem::take(&mut self.xi[set]);
		let (xi_x, rest) = memory.split_at_mut(strips.x_len());
		let delta = &rest[strips.len() - strips.x_len()..];
		let (pml, psi) = (&*self, &self.psi[set][..strips.x_len()]);
		let rows = &mut out[PAD * s..(PAD + gh) * s];
		rows.par_chunks_mut(s).zip(xi_x.par_chunks_mut(2 * rim)).enumerate().for_each(|(gy, (line, xi_x))| {
			let delta_row = Self::strip_row(strips, gy).map(|r| &delta[r * gw..][..gw]);
			let zrow = &pml.z[gy * zw..][..zw];
			let psi_row = &psi[gy * 2 * rim..][..2 * rim];
			let base = (gy + PAD) * s + PAD;
			let mut cell = |gx: usize, sum: &mut f32, col: Option<usize>| {
				if let Some(col) = col {
					let half = if col < rim { 0 } else { rim };
					let p = |k: isize| {
						let q = col as isize + k;
						if q >= half as isize && q < (half + rim) as isize { psi_row[q as usize] } else { 0.0 }
					};
					let dpsi = (8.0 * (p(1) - p(-1)) - (p(2) - p(-2))) / 12.0;
					let (_, zc) = Self::x_column(gw, rim, col);
					let taps = cut_taps(std::array::from_fn(|k| zrow[zc + k - 3]), cuts[base + gx] >> cut::X_SHIFT);
					*sum += stretched(&taps, &pml.axis, pml.x[gx], dpsi, &mut xi_x[col]);
				}
			};
			let mut add = |gx: usize, sum: f32| line[gx + PAD] += scale * c2[base + gx] * sum;
			let x_col = |gx: usize| {
				if gx < rim {
					Some(gx)
				} else if gx >= gw - rim {
					Some(gx + 2 * rim - gw)
				} else {
					None
				}
			};
			match delta_row {
				Some(d) => {
					for gx in 0..gw {
						let mut sum = d[gx];
						if pml.cross != 0.0 {
							let left = if gx > 0 { d[gx - 1] } else { 0.0 };
							let right = if gx + 1 < gw { d[gx + 1] } else { 0.0 };
							sum += pml.cross * (left + right - 2.0 * d[gx]);
						}
						cell(gx, &mut sum, x_col(gx));
						add(gx, sum);
					}
				}
				None => {
					for gx in (0..rim).chain(gw - rim..gw) {
						let mut sum = 0.0;
						cell(gx, &mut sum, x_col(gx));
						add(gx, sum);
					}
				}
			}
		});
		self.xi[set] = memory;
	}
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
