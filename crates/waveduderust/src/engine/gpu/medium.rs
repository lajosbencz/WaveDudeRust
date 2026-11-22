//! The scene's c2 and damping maps in the GPU's layout, at the sub-step scale.
//!
//! Values are the CPU engine's: `c2 / n^2` and `d^(1/n)` in f32 per cell (`SceneData::fill_sub_step_medium`).
//! One u32 per cell covers the padded grid ([`SceneData::stride`]): a 16-bit index into the c2 table, a 16-bit
//! index into the damping table. The padding's entries are never read. The mirrors' cut bytes come with them.

use std::collections::HashMap;

use super::GpuError;
use crate::engine::{SceneData, pml};

/// Distinct values a table can index (16-bit indices).
const TABLE_LIMIT: usize = 1 << 16;

/// One built medium: per-cell words and the tables (c2 first, damping from `d_base`).
pub(super) struct Medium {
	pub cells: Vec<u32>,
	/// c2 table then damping table.
	pub tables: Vec<f32>,
	pub d_base: u32,
	/// Padded c2 and d at the sub-step scale (for the Mur weights).
	pub c2: Vec<f32>,
	pub d: Vec<f32>,
	/// Cut bytes per padded cell (engine/pml.rs, `cuts`); all 0 without mirrors.
	pub cuts: Vec<u8>,
}

impl Medium {
	/// Whether the kernels apply the cuts.
	pub(super) fn mirrors(&self) -> bool {
		self.cuts.iter().any(|&c| c != 0)
	}
}

/// The medium of `scene` for `n` sub-steps; refused above [`TABLE_LIMIT`] distinct values of c2 or damping.
pub(super) fn build(scene: &SceneData, n: u32) -> Result<Medium, GpuError> {
	let cells = scene.padded_cells();
	let mut c2 = vec![0f32; cells];
	let mut d = vec![1f32; cells];
	scene.fill_sub_step_medium(n, &mut c2, &mut d);
	let (cells, tables, d_base) = compact(scene, &c2, &d).ok_or(GpuError::MediumTables(TABLE_LIMIT))?;
	Ok(Medium { cells, tables, d_base, c2, d, cuts: pml::cuts(scene) })
}

/// The words of the grid cells and the tables (c2, then damping from the returned base); `None` above
/// [`TABLE_LIMIT`] distinct values of either.
fn compact(scene: &SceneData, c2: &[f32], d: &[f32]) -> Option<(Vec<u32>, Vec<f32>, u32)> {
	let (gw, gh) = (scene.grid_width() as usize, scene.grid_height() as usize);
	let mut c2_index: HashMap<u32, u16> = HashMap::new();
	let mut d_index: HashMap<u32, u16> = HashMap::new();
	let (mut c2_table, mut d_table) = (Vec::new(), Vec::new());
	let mut words = vec![0u32; c2.len()];
	// A scene is runs of equal cells: a run is interned once.
	let mut last: Option<((u32, u32), u32)> = None;
	for gy in 0..gh {
		let at = scene.padded_index(0, gy);
		for i in at..at + gw {
			let key = (c2[i].to_bits(), d[i].to_bits());
			words[i] = match last {
				Some((k, word)) if k == key => word,
				_ => {
					let ci = intern(&mut c2_index, &mut c2_table, c2[i])?;
					let di = intern(&mut d_index, &mut d_table, d[i])?;
					let word = ci as u32 | (di as u32) << 16;
					last = Some((key, word));
					word
				}
			};
		}
	}
	let d_base = c2_table.len() as u32;
	c2_table.extend(d_table);
	Some((words, c2_table, d_base))
}

/// Index of `v` (by bits) in `table`, added when new; `None` once the table is full.
fn intern(index: &mut HashMap<u32, u16>, table: &mut Vec<f32>, v: f32) -> Option<u16> {
	if let Some(&i) = index.get(&v.to_bits()) {
		return Some(i);
	}
	if table.len() == TABLE_LIMIT {
		return None;
	}
	let i = table.len() as u16;
	table.push(v);
	index.insert(v.to_bits(), i);
	Some(i)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Scene;

	#[test]
	fn the_tables_hold_the_sub_step_values() {
		let scene = SceneData::blank(10, 6, &Scene { damping_edge: -4, ..Scene::default() });
		let m = build(&scene, 2).unwrap();
		let first = scene.padded_index(0, 0);
		for (cell, &w) in m.cells.iter().enumerate().skip(first).take(17) {
			let (c2, d) = (m.tables[(w & 0xffff) as usize], m.tables[m.d_base as usize + (w >> 16) as usize]);
			assert_eq!(c2, m.c2[cell]);
			let gx = cell - first;
			assert_eq!(d, (scene.d[gx] as f64).powf(0.5) as f32);
		}
		assert_eq!(&m.tables[..m.d_base as usize], &[0.25]);
	}

	#[test]
	fn too_many_values_are_refused() {
		let mut scene = SceneData::blank(300, 300, &Scene { damping_edge: 0, ..Scene::default() });
		for (i, c) in scene.c2.iter_mut().enumerate() {
			*c = 0.5 + i as f32 * 1e-6;
		}
		assert!(matches!(build(&scene, 1), Err(GpuError::MediumTables(TABLE_LIMIT))));
	}
}
