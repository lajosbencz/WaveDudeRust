//! Sources as a forcing term of the wave equation, shared by both engines (docs/physics.md, Sources).
//!
//! A source of amplitude A, angular frequency omega (rad/step), phase phi and envelope e(t) adds
//! `f(t) = A / |G(omega)| e(t) sin(omega t + phi)` to `u_tt`, G the free-space response of the
//! engine's discrete operator at the source's position (engine/green.rs), so a continuous source in free
//! space (index 1) swings its position with amplitude A. A source at a fractional position drives the
//! 2 x 2 cells around it with bilinear shares (its taps), and G is the response sampled with the same
//! shares. Each sub-step of `dt = 1/n` takes `F = dt^2 f(t_k)` at the time `t_k` of its
//! current field where the scheme takes `A u`; the modified equation also takes `dt^4 f_tt / 12` with
//! `f_tt = -omega^2 f` (the envelope's own derivatives neglected), folded in as
//! `F (1 - (omega dt)^2 / 12)`.

use std::collections::HashMap;

use super::envelope::{Pulse, detach_fade, level_omega};
use super::green::spread_response;
use super::stencil::{StencilCoefficients, TimeScheme};
use super::types::{Emitter, SceneData};

/// Entries of the value table before the object emitters' taps: one per image level.
pub const IMAGE_LEVELS: usize = 256;

/// One cell an emitter drives, with its bilinear share of the drive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
	/// Padded grid index.
	pub cell: usize,
	pub emitter: usize,
	pub share: f64,
}

/// A value with these bits drives nothing (a detached emitter).
pub const SKIP_VALUE: u32 = u32::MAX;

/// Whether a table value adds to its cell: not skipped and not zero, so a silent or detached source
/// leaves the field's bits as they are.
#[inline]
pub fn live(v: f32) -> bool {
	v.to_bits() != SKIP_VALUE && v != 0.0
}

/// `dt^2` times the scheme's factor for a source of angular frequency `omega` (rad/step) at `n`
/// sub-steps a step.
pub fn force_scale(omega: f64, n: u32, scheme: TimeScheme) -> f64 {
	let w = omega / n as f64;
	match scheme {
		TimeScheme::Leapfrog => 1.0 / (n as f64 * n as f64),
		TimeScheme::ModifiedEquation => (1.0 - w * w / 12.0) / (n as f64 * n as f64),
	}
}

/// The per-step source state an engine keeps: the level table, the emitters (those on the canvas)
/// with their taps and pulses, and the normalisations computed so far.
pub struct Sources {
	pub stencil: StencilCoefficients,
	/// `F / |G|` per unit drive by sub-step count, angular frequency bits and the position's fraction bits.
	norms: HashMap<(u32, u64, u64, u64), f64>,
	pub level_omega: [f64; IMAGE_LEVELS],
	pub level_used: [bool; IMAGE_LEVELS],
	pub image_scale: f64,
	pub frequency_scale: f64,
	pub emitters: Vec<Emitter>,
	pub taps: Vec<Tap>,
	pub pulses: Vec<Pulse>,
	/// Scratch: each emitter's drive of the current sub-step, None once detached.
	drives: Vec<Option<f64>>,
}

impl Sources {
	pub fn new(stencil: StencilCoefficients, frequency_scale: f64, image_scale: f64) -> Self {
		Self {
			stencil,
			norms: HashMap::new(),
			level_omega: std::array::from_fn(|l| level_omega(l as f64, frequency_scale)),
			level_used: [false; IMAGE_LEVELS],
			image_scale,
			frequency_scale,
			emitters: Vec::new(),
			taps: Vec::new(),
			pulses: Vec::new(),
			drives: Vec::new(),
		}
	}

	/// EMA weight of the intensity from the lowest frequency level in use: `exp(-1/tau)`,
	/// `tau = max(1, time_decay * 100 / lowest)`, the lowest level 100 without sources.
	pub fn intensity_alpha(&self, time_decay: f64) -> f32 {
		debug_assert!(self.emitters.iter().all(|e| e.level > 0.0), "emitter levels of at least MIN_LEVEL");
		let image = (1..IMAGE_LEVELS).find(|&l| self.level_used[l]).map(|l| l as f64);
		let object = self.emitters.iter().map(|e| e.level).reduce(f64::min);
		let lowest = match (image, object) {
			(Some(a), Some(b)) => a.min(b),
			(Some(a), None) | (None, Some(a)) => a,
			(None, None) => 100.0,
		};
		let tau = (time_decay * 100.0 / lowest).max(1.0);
		(-1.0 / tau).exp() as f32
	}

	/// Keeps the emitters that drive a cell of `scene`'s canvas, with their taps as padded indices, and
	/// restarts their pulses.
	pub fn set_emitters(&mut self, emitters: &[Emitter], scene: &SceneData) {
		let rim = scene.rim as usize;
		self.emitters = emitters.iter().filter(|e| e.inside(scene)).cloned().collect();
		self.taps = self
			.emitters
			.iter()
			.enumerate()
			.flat_map(|(k, e)| {
				e.taps(scene).map(move |((x, y), share)| Tap {
					cell: scene.padded_index(x + rim, y + rim),
					emitter: k,
					share,
				})
			})
			.collect();
		self.reset_pulses();
	}

	/// The driven cells in tap order (image sources come before them in the value table).
	pub fn tap_cells(&self) -> Vec<usize> {
		self.taps.iter().map(|t| t.cell).collect()
	}

	/// Length of the value table: the image levels, then one entry per tap.
	pub fn table_len(&self) -> usize {
		IMAGE_LEVELS + self.taps.len()
	}

	pub fn reset_pulses(&mut self) {
		self.pulses = self.emitters.iter().map(Pulse::new).collect();
	}

	/// `F` per unit drive of a source of angular frequency `omega` at `n` sub-steps, spread by the
	/// fractional position `fraction`: `force_scale / |G|`, cached; 0 for omega <= 0 (|G| grows without
	/// bound there).
	pub fn unit_force(&mut self, omega: f64, n: u32, scheme: TimeScheme, fraction: (f64, f64)) -> f64 {
		if omega.is_nan() || omega <= 0.0 {
			return 0.0;
		}
		let stencil = self.stencil;
		*self.norms.entry((n, omega.to_bits(), fraction.0.to_bits(), fraction.1.to_bits())).or_insert_with(|| {
			force_scale(omega, n, scheme) / spread_response(&stencil, scheme, n, omega, 1.0, fraction)
		})
	}

	/// The table of one sub-step at time `t` (steps): `F` per image level in use, then per tap its
	/// emitter's drive times its share ([`SKIP_VALUE`] once the emitter is detached). Advances the
	/// pulse states, so `t` must not decrease.
	pub fn values(&mut self, out: &mut [f32], t: f64, n: u32, scheme: TimeScheme) {
		assert!(out.len() >= self.table_len());
		for (level, slot) in out[..IMAGE_LEVELS].iter_mut().enumerate() {
			*slot = 0.0;
			if self.level_used[level] {
				let omega = self.level_omega[level];
				*slot = (self.unit_force(omega, n, scheme, (0.0, 0.0)) * (omega * t).sin() * self.image_scale) as f32;
			}
		}
		self.drives.clear();
		for k in 0..self.emitters.len() {
			let e = self.emitters[k].clone();
			let drive = detach_fade(&e, t).map(|fade| {
				let omega = level_omega(e.level, self.frequency_scale);
				let drive = (e.phase_radians + omega * t).sin() * e.amplitude * self.pulses[k].envelope(&e, omega, t);
				self.unit_force(omega, n, scheme, e.fraction()) * drive * fade as f64
			});
			self.drives.push(drive);
		}
		for (k, tap) in self.taps.iter().enumerate() {
			out[IMAGE_LEVELS + k] = match self.drives[tap.emitter] {
				None => f32::from_bits(SKIP_VALUE),
				Some(drive) => (drive * tap.share) as f32,
			};
		}
	}
}

/// The driven cells, each once in order of first appearance (image sources first, then the emitters'
/// taps), with the table entries that drive it in source order. A source on a mirror cell drives nothing:
/// the cell is a wall with no wave coupling, where a force would only push it off (a free mass
/// driven from rest drifts by `omega A cos(phi)` a step).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceGroups {
	pub cells: Vec<usize>,
	/// Start and length of each cell's run in `entries`.
	pub runs: Vec<(u32, u32)>,
	pub entries: Vec<u32>,
}

impl SourceGroups {
	/// Groups of image sources `(cell, level)` and tap cells (padded indices) of `scene`.
	pub fn new(scene: &SceneData, image: impl IntoIterator<Item = (usize, u8)>, taps: &[usize]) -> Self {
		let mut order = Vec::new();
		let mut by_cell: HashMap<usize, Vec<u32>> = HashMap::new();
		let image = image.into_iter().map(|(c, l)| (c, l as u32));
		let object = taps.iter().enumerate().map(|(k, &c)| (c, (IMAGE_LEVELS + k) as u32));
		for (cell, entry) in image.chain(object).filter(|&(c, _)| !scene.is_mirror_at(c)) {
			by_cell
				.entry(cell)
				.or_insert_with(|| {
					order.push(cell);
					Vec::new()
				})
				.push(entry);
		}
		let mut groups = Self::default();
		for cell in order {
			let run = &by_cell[&cell];
			groups.runs.push((groups.entries.len() as u32, run.len() as u32));
			groups.entries.extend_from_slice(run);
			groups.cells.push(cell);
		}
		groups
	}

	/// Each cell with its run of entries.
	pub fn iter(&self) -> impl Iterator<Item = (usize, &[u32])> {
		self.cells.iter().zip(&self.runs).map(|(&c, &(s, l))| (c, &self.entries[s as usize..(s + l) as usize]))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn groups_keep_first_appearance_and_source_order_and_skip_mirrors() {
		let mut scene = SceneData::blank(8, 8, &crate::model::Scene { damping_edge: 0, ..Default::default() });
		let at = |x: usize, y: usize| scene.padded_index(x, y);
		let (a, b, c, m) = (at(1, 1), at(2, 1), at(3, 1), at(4, 1));
		scene.c2[4 + 8] = 0.0;
		let g = SourceGroups::new(&scene, [(a, 5), (b, 7), (a, 9), (m, 3)], &[b, c, a, m]);
		let runs: Vec<(usize, Vec<u32>)> = g.iter().map(|(c, e)| (c, e.to_vec())).collect();
		assert_eq!(runs, vec![(a, vec![5, 9, 258]), (b, vec![7, 256]), (c, vec![257])]);
	}

	#[test]
	fn the_fourth_order_factor_is_the_source_tttt_term() {
		let (omega, n) = (0.4, 2);
		assert_eq!(force_scale(omega, n, TimeScheme::Leapfrog), 0.25);
		assert!((force_scale(omega, n, TimeScheme::ModifiedEquation) - 0.25 * (1.0 - 0.04 / 12.0)).abs() < 1e-15);
		assert!(!live(0.0) && !live(-0.0) && !live(f32::from_bits(SKIP_VALUE)) && live(1e-30));
	}
}
