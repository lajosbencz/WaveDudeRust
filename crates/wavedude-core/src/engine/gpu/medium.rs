//! The scene's c2 and damping maps in the GPU's layout, at the sub-step scale.
//!
//! Values are the CPU engine's: `c2 / n^2` and `d^(1/n)` in f32 per cell (engine/cpu.rs,
//! `load_scene`). Both layouts cover the padded grid ([`SceneData::stride`]); the padding's entries
//! are never read.

use std::collections::HashMap;

use crate::engine::SceneData;

/// Distinct values a compact table can index (16-bit indices).
const TABLE_LIMIT: usize = 1 << 16;

/// How the medium reaches the card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediumLayout {
	/// One u32 per cell: a 16-bit index into the c2 table, a 16-bit index into the damping table
	/// (4 bytes a cell). Falls back to [`Plain`](Self::Plain) above 65536 distinct values of either.
	#[default]
	Compact,
	/// (c2, d) as two f32 per cell (8 bytes a cell).
	Plain,
}

/// One built medium: per-cell words and the tables (c2 first, damping from `d_base`).
pub struct Medium {
	pub layout: MediumLayout,
	/// u32 per cell (compact) or two f32 per cell (plain), as bytes.
	pub cells: Vec<u8>,
	/// c2 table then damping table; one zero for the plain layout.
	pub tables: Vec<f32>,
	pub d_base: u32,
	/// Padded c2 and d at the sub-step scale (for the Mur weights).
	pub c2: Vec<f32>,
	pub d: Vec<f32>,
}

/// The medium of `scene` for `n` sub-steps in `layout`, compact falling back to plain.
pub fn build(scene: &SceneData, n: u32, layout: MediumLayout) -> Medium {
	let gw = scene.grid_width() as usize;
	let cells = scene.padded_cells();
	let mult = 1.0 / (n as f32 * n as f32);
	let root = 1.0 / n as f64;
	let mut c2 = vec![0f32; cells];
	let mut d = vec![1f32; cells];
	let mut c2_index: HashMap<u32, u16> = HashMap::new();
	let mut d_index: HashMap<u32, u16> = HashMap::new();
	let (mut c2_table, mut d_table) = (Vec::new(), Vec::new());
	let mut words = vec![0u32; cells];
	let mut compact = layout == MediumLayout::Compact;
	for (gy, (rc2, rd)) in scene.c2.chunks_exact(gw).zip(scene.d.chunks_exact(gw)).enumerate() {
		let at = scene.padded_index(0, gy);
		for (k, (&c, &dd)) in rc2.iter().zip(rd).enumerate() {
			let (cv, dv) = (c * mult, if n > 1 { (dd as f64).powf(root) as f32 } else { dd });
			c2[at + k] = cv;
			d[at + k] = dv;
			if compact {
				let ci = intern(&mut c2_index, &mut c2_table, cv);
				let di = intern(&mut d_index, &mut d_table, dv);
				match (ci, di) {
					(Some(ci), Some(di)) => words[at + k] = ci as u32 | (di as u32) << 16,
					_ => compact = false,
				}
			}
		}
	}
	if compact {
		let d_base = c2_table.len() as u32;
		c2_table.extend(d_table);
		return Medium {
			layout: MediumLayout::Compact,
			cells: bytemuck::cast_slice(&words).to_vec(),
			tables: c2_table,
			d_base,
			c2,
			d,
		};
	}
	let pairs: Vec<[f32; 2]> = c2.iter().zip(&d).map(|(&c, &d)| [c, d]).collect();
	Medium {
		layout: MediumLayout::Plain,
		cells: bytemuck::cast_slice(&pairs).to_vec(),
		tables: vec![0.0],
		d_base: 0,
		c2,
		d,
	}
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
	fn compact_tables_hold_the_sub_step_values() {
		let scene = SceneData::blank(10, 6, &Scene { damping_edge: -4, ..Scene::default() });
		let m = build(&scene, 2, MediumLayout::Compact);
		assert_eq!(m.layout, MediumLayout::Compact);
		let words: &[u32] = bytemuck::cast_slice(&m.cells);
		let first = scene.padded_index(0, 0);
		for (cell, &w) in words.iter().enumerate().skip(first).take(17) {
			let (c2, d) = (m.tables[(w & 0xffff) as usize], m.tables[m.d_base as usize + (w >> 16) as usize]);
			assert_eq!(c2, m.c2[cell]);
			let gx = cell - first;
			assert_eq!(d, (scene.d[gx] as f64).powf(0.5) as f32);
		}
		assert_eq!(&m.tables[..m.d_base as usize], &[0.25]);
	}

	#[test]
	fn too_many_values_fall_back_to_plain() {
		let mut scene = SceneData::blank(300, 300, &Scene { damping_edge: 0, ..Scene::default() });
		for (i, c) in scene.c2.iter_mut().enumerate() {
			*c = 0.5 + i as f32 * 1e-6;
		}
		let m = build(&scene, 1, MediumLayout::Compact);
		assert_eq!(m.layout, MediumLayout::Plain);
		assert_eq!(m.cells.len(), 306 * 306 * 8);
	}
}
