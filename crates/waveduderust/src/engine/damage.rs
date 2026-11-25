//! The rectangle of grid cells a CPU pass runs on (engine/cpu.rs, Damage).

use std::ops::Range;

use super::stencil::PAD;

/// The rectangle of grid cells `x` by `y` that may hold a value (engine/cpu.rs, Damage); empty when either range is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Damage {
	pub x: Range<usize>,
	pub y: Range<usize>,
}

impl Damage {
	pub const NONE: Self = Self { x: 0..0, y: 0..0 };

	/// The whole `gw` x `gh` grid.
	pub fn all(gw: usize, gh: usize) -> Self {
		Self { x: 0..gw, y: 0..gh }
	}

	pub fn is_empty(&self) -> bool {
		self.x.is_empty() || self.y.is_empty()
	}

	/// Each side grown by `by` cells within the `gw` x `gh` grid; nothing stays nothing.
	pub fn grown(&self, by: usize, gw: usize, gh: usize) -> Self {
		if self.is_empty() {
			return Self::NONE;
		}
		let grown = |r: &Range<usize>, end: usize| r.start.saturating_sub(by)..(r.end + by).min(end);
		Self { x: grown(&self.x, gw), y: grown(&self.y, gh) }
	}

	/// Takes in the grid cell at padded index `cell` (row length `s`).
	pub fn include(&mut self, cell: usize, s: usize) {
		self.include_cell(cell % s - PAD, cell / s - PAD);
	}

	/// Takes in grid cell (`gx`, `gy`).
	pub fn include_cell(&mut self, gx: usize, gy: usize) {
		*self = if self.is_empty() {
			Self { x: gx..gx + 1, y: gy..gy + 1 }
		} else {
			Self { x: self.x.start.min(gx)..self.x.end.max(gx + 1), y: self.y.start.min(gy)..self.y.end.max(gy + 1) }
		};
	}
}

/// `known` with every cell of `area` (which holds it) where the padded `buf` (row length `s`) is not +0. Only the band
/// of `area` outside `known` is read.
pub fn extent(buf: &[f32], s: usize, area: &Damage, known: Damage) -> Damage {
	if *area == known {
		return known;
	}
	let mut out = known.clone();
	let mut scan = |gy: usize, xs: Range<usize>| {
		let at = (gy + PAD) * s + PAD;
		let row = &buf[at..][xs.clone()];
		if let Some(first) = row.iter().position(|v| v.to_bits() != 0) {
			let last = row.iter().rposition(|v| v.to_bits() != 0).unwrap_or(first);
			out.include_cell(xs.start + first, gy);
			out.include_cell(xs.start + last, gy);
		}
	};
	for gy in area.y.clone() {
		if known.is_empty() || !known.y.contains(&gy) {
			scan(gy, area.x.clone());
		} else {
			scan(gy, area.x.start..known.x.start);
			scan(gy, known.x.end..area.x.end);
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn damage_grows_within_the_grid() {
		let mut d = Damage::NONE;
		assert_eq!(d.grown(3, 10, 8), Damage::NONE);
		d.include((PAD + 2) * 16 + PAD + 8, 16);
		assert_eq!(d, Damage { x: 8..9, y: 2..3 });
		d = d.grown(3, 10, 8);
		assert_eq!(d, Damage { x: 5..10, y: 0..6 });
		d.include((PAD + 7) * 16 + PAD, 16);
		assert_eq!(d, Damage { x: 0..10, y: 0..8 });
	}
}
