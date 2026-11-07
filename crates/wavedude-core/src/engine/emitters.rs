//! Object sources to engine emitters (spec/10-model.md, Element generation and Emitter).

use std::f64::consts::PI;
use std::num::NonZeroU32;

use super::green::bilinear;
use super::types::{Emitter, SceneData};
use crate::model::{Project, Source, SourceMode};
use crate::rng::{Pcg32, stable_hash};

/// Stream selector XOR-ed onto the name hash for the gap seeds.
const GAP_STREAM: u64 = 0x47415053;

impl Emitter {
	/// Whether the emitter drives any cell of the canvas of `scene`; engines drop the others.
	pub fn inside(&self, scene: &SceneData) -> bool {
		self.taps(scene).next().is_some()
	}

	/// Fractional parts of the position, the bilinear offsets of the taps.
	pub fn fraction(&self) -> (f64, f64) {
		(self.x - self.x.floor(), self.y - self.y.floor())
	}

	/// The canvas cells the emitter drives with their bilinear shares: the one cell at a whole-number
	/// position, up to four around a fractional one; cells off the canvas are dropped, which keeps the
	/// shares of the rest (a source at the edge drives what is there).
	pub fn taps(&self, scene: &SceneData) -> impl Iterator<Item = ((usize, usize), f64)> + use<> {
		let (x0, y0) = (self.x.floor(), self.y.floor());
		let (fx, fy) = self.fraction();
		let shares = bilinear(fx, fy);
		let (w, h) = (scene.canvas_width as i64, scene.canvas_height as i64);
		[(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().zip(shares).filter_map(move |((dx, dy), share)| {
			let (x, y) = (x0 as i64 + dx, y0 as i64 + dy);
			(share > 0.0 && (0..w).contains(&x) && (0..h).contains(&y)).then_some(((x as usize, y as usize), share))
		})
	}
}

/// Least frequency level of an element.
pub const MIN_LEVEL: f64 = 0.01;

/// One generated element of a source: position in cells (as [`Emitter`]), level, degrees and cycles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Element {
	pub x: f64,
	pub y: f64,
	pub level: f64,
	pub phase: f64,
	pub off_width: u32,
}

fn jitter(rng: &mut Pcg32, amount: u32) -> i32 {
	if amount == 0 {
		0
	} else {
		let a = i32::try_from(amount).expect("jitter amounts are normalised to i32::MAX");
		rng.next_range(-a, a)
	}
}

/// The elements of `source` for the project seed `seed`: one without an array, else the array's
/// grid row by row with jitter drawn in the order x, y, level, phase, off width.
pub fn elements_of(source: &Source, seed: i32) -> Vec<Element> {
	let Some(array) = &source.array else {
		return vec![Element {
			x: source.x,
			y: source.y,
			level: source.frequency,
			phase: source.phase,
			off_width: source.off_width,
		}];
	};
	let mut rng = Pcg32::new(seed as u32 as u64, stable_hash(&source.name) as u64);
	let a = array.angle * PI / 180.0;
	let (sin, cos) = a.sin_cos();
	let mut out = Vec::with_capacity(array.size_x as usize * array.size_y as usize);
	for row in 0..array.size_y {
		for col in 0..array.size_x {
			let dx = (col as f64 - (array.size_x as f64 - 1.0) / 2.0) * array.spacing_x as f64;
			let dy = (row as f64 - (array.size_y as f64 - 1.0) / 2.0) * array.spacing_y as f64;
			// Whole-cell offsets from the source's position, so an array keeps the original's layout.
			let mut x = source.x + (dx * cos + dy * sin).round_ties_even();
			let mut y = source.y + (dy * cos - dx * sin).round_ties_even();
			x += f64::from(jitter(&mut rng, array.random_position));
			y += f64::from(jitter(&mut rng, array.random_position));
			let level = (source.frequency + jitter(&mut rng, array.random_frequency) as f64).clamp(MIN_LEVEL, 255.0);
			let phase = (source.phase + jitter(&mut rng, array.random_phase) as f64).clamp(-180.0, 180.0);
			let off = (source.off_width as i64 + jitter(&mut rng, array.random_off_width) as i64).max(1);
			let off_width = u32::try_from(off).expect("off width and jitter are normalised to i32::MAX each");
			out.push(Element { x, y, level, phase, off_width });
		}
	}
	out
}

/// The emitters of one enabled source; empty for a disabled one.
pub fn emitters_of_source(source: &Source, seed: i32) -> Vec<Emitter> {
	if !source.enabled {
		return Vec::new();
	}
	let mut rng = Pcg32::new(seed as u32 as u64, stable_hash(&source.name) as u64 ^ GAP_STREAM);
	elements_of(source, seed)
		.into_iter()
		.map(|el| Emitter {
			x: el.x,
			y: el.y,
			level: el.level,
			amplitude: source.amplitude / 100.0,
			phase_radians: el.phase * PI / 180.0,
			pulsed: source.mode == SourceMode::Pulsed,
			shape: source.pulse_shape,
			on_width_cycles: source.pulse_width,
			off_width_cycles: el.off_width,
			off_width_base: source.off_width,
			randomize_off_width: source.array.as_ref().map_or(0, |a| a.random_off_width),
			detach_after_step: source.detach_after,
			rng_seed: NonZeroU32::new(rng.next_u32()).unwrap_or(NonZeroU32::MIN),
		})
		.collect()
}

/// Every emitter of the project in source order; none when the scene's object sources are off.
/// Expects a normalised project.
pub fn emitters_of(project: &Project) -> Vec<Emitter> {
	if !project.scene.object_sources {
		return Vec::new();
	}
	project.sources.iter().flat_map(|s| emitters_of_source(s, project.seed)).collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::SourceArray;

	fn source(name: &str) -> Source {
		Source { name: name.into(), x: 50.0, y: 40.0, ..Source::default() }
	}

	#[test]
	fn a_plain_source_is_one_emitter_without_draws() {
		let mut s = source("Source 1");
		s.amplitude = 50.0;
		s.phase = 90.0;
		let e = emitters_of_source(&s, 7);
		assert_eq!(e.len(), 1);
		assert_eq!((e[0].x, e[0].y, e[0].level), (50.0, 40.0, 100.0));
		assert_eq!(e[0].amplitude, 0.5);
		assert!((e[0].phase_radians - PI / 2.0).abs() < 1e-15);
		assert_eq!(e[0].randomize_off_width, 0);
		let expected = Pcg32::new(7, stable_hash("Source 1") as u64 ^ GAP_STREAM).next_u32().max(1);
		assert_eq!(e[0].rng_seed.get(), expected);
	}

	#[test]
	fn a_one_by_one_array_draws_jitter() {
		let mut s = source("A");
		s.array = Some(SourceArray { random_position: 5, ..SourceArray::default() });
		let el = elements_of(&s, 3);
		let mut rng = Pcg32::new(3, stable_hash("A") as u64);
		let dx = rng.next_range(-5, 5);
		let dy = rng.next_range(-5, 5);
		assert_eq!((el[0].x, el[0].y), (50.0 + f64::from(dx), 40.0 + f64::from(dy)));
	}

	#[test]
	fn array_grid_is_row_major_and_rotated() {
		let mut s = source("B");
		s.array = Some(SourceArray {
			size_x: 3,
			size_y: 2,
			spacing_x: 10,
			spacing_y: 20,
			angle: 90.0,
			..SourceArray::default()
		});
		let el = elements_of(&s, 1);
		assert_eq!(el.len(), 6);
		// 90 degrees CCW on screen: +x goes to -y.
		assert_eq!((el[0].x, el[0].y), (40.0, 50.0));
		assert_eq!((el[2].x, el[2].y), (40.0, 30.0));
		assert_eq!((el[3].x, el[3].y), (60.0, 50.0));
	}

	#[test]
	fn levels_and_phases_are_clamped() {
		let mut s = source("C");
		s.frequency = 0.01;
		s.phase = 180.0;
		s.array = Some(SourceArray { size_x: 20, random_frequency: 5, random_phase: 30, ..SourceArray::default() });
		for el in elements_of(&s, 9) {
			assert!(el.level >= MIN_LEVEL && el.phase <= 180.0);
		}
	}

	#[test]
	fn taps_spread_a_fractional_position_and_stop_at_the_canvas() {
		let scene = SceneData::blank(8, 8, &crate::model::Scene { damping_edge: 0, ..Default::default() });
		let at = |x: f64, y: f64| Emitter { x, y, ..emitters_of_source(&source("T"), 1)[0].clone() };
		let whole: Vec<_> = at(3.0, 4.0).taps(&scene).collect();
		assert_eq!(whole, vec![((3, 4), 1.0)]);
		let half: Vec<_> = at(3.5, 4.0).taps(&scene).collect();
		assert_eq!(half, vec![((3, 4), 0.5), ((4, 4), 0.5)]);
		let corner: Vec<_> = at(3.25, 4.5).taps(&scene).collect();
		assert_eq!(corner, vec![((3, 4), 0.375), ((4, 4), 0.125), ((3, 5), 0.375), ((4, 5), 0.125)]);
		let edge: Vec<_> = at(-0.5, 2.0).taps(&scene).collect();
		assert_eq!(edge, vec![((0, 2), 0.5)]);
		assert!(!at(-1.0, 2.0).inside(&scene) && at(7.5, 7.5).inside(&scene) && !at(8.0, 2.0).inside(&scene));
	}

	#[test]
	fn gates_and_disabled_sources() {
		let mut p = Project::default();
		p.sources.push(source("On"));
		p.sources.push(Source { enabled: false, ..source("Off") });
		assert_eq!(emitters_of(&p).len(), 1);
		p.scene.object_sources = false;
		assert!(emitters_of(&p).is_empty());
	}
}
