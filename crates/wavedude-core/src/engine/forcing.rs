//! Sources as a forcing term of the wave equation, shared by both engines (docs/physics.md, Sources).
//!
//! A source of amplitude A, angular frequency omega (rad/step), phase phi and envelope E(t) adds
//! `f = dp/dt` to `u_tt`, its impulse `p(t) = -A / |G(omega)| E(t) cos(omega t + phi) / omega`, G the
//! free-space response of the engine's discrete operator at the source's position (engine/green.rs): while
//! E = 1, `f = A / |G| sin(omega t + phi)`, so a continuous source in free space (index 1) swings its
//! position with amplitude A, and a drive whose envelope is back at 0 has delivered no impulse, leaving no
//! zero-frequency field behind. E is the turn-on times the pulse envelope times the detach fade
//! (engine/envelope.rs). A source at a fractional position drives the 2 x 2 cells around it with bilinear
//! shares (its taps), and G is the response sampled with the same shares. Each sub-step of `dt = 1/n` takes
//! `F = dt^2 f(t_k)` at the time `t_k` of its current field where the scheme takes `A u`, f the difference
//! of p over the sub-step; the modified equation also takes `dt^4 f_tt / 12` with `f_tt = -omega^2 f` (the
//! envelope's own derivatives neglected), folded in as `F (1 - (omega dt)^2 / 12)`.

use std::collections::HashMap;

use super::cpu::sub_step_time;
use super::envelope::{Pulse, detach_fade, turn_on};
use super::green::spread_response;
use super::stencil::{StencilCoefficients, TimeScheme};
use super::types::{Emitter, SceneData};
use crate::model::level_omega;

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

	/// The table of sub-step `k` of `n` after `time` steps: `F` per image level in use, then per tap its
	/// emitter's drive times its share ([`SKIP_VALUE`] once the emitter is detached). Advances the
	/// pulse states, so the sub-steps must come in order.
	pub fn values(&mut self, out: &mut [f32], time: u32, k: u32, n: u32, scheme: TimeScheme) {
		assert!(out.len() >= self.table_len());
		let t = sub_step_time(time, k, n);
		let dt = 1.0 / n as f64;
		// Half a sub-step either side, from one integer count so that a sub-step's end is its successor's start.
		let half = |m: i64| (2 * n as i64 * time as i64 + 2 * k as i64 + m) as f64 / (2 * n) as f64;
		let (lo, hi) = (half(-1), half(1));
		for (level, slot) in out[..IMAGE_LEVELS].iter_mut().enumerate() {
			*slot = 0.0;
			if self.level_used[level] {
				let omega = self.level_omega[level];
				let ramp = (turn_on(lo, omega), turn_on(hi, omega));
				let drive = impulse_drive(omega, 0.0, t, dt, ramp);
				*slot = (self.unit_force(omega, n, scheme, (0.0, 0.0)) * drive * self.image_scale) as f32;
			}
		}
		self.drives.clear();
		for i in 0..self.emitters.len() {
			let e = self.emitters[i].clone();
			let omega = level_omega(e.level, self.frequency_scale);
			let (fade_lo, fade_hi) = (detach_fade(&e, lo), detach_fade(&e, hi));
			let drive = (fade_lo.is_some() || fade_hi.is_some()).then(|| {
				let pulse = &mut self.pulses[i];
				let envelope = |fade: Option<f64>, at: f64, pulse: &mut Pulse| {
					fade.unwrap_or(0.0) * turn_on(at, omega) * pulse.envelope(&e, omega, at)
				};
				let ends = (envelope(fade_lo, lo, pulse), envelope(fade_hi, hi, pulse));
				let drive = impulse_drive(omega, e.phase_radians, t, dt, ends);
				self.unit_force(omega, n, scheme, e.fraction()) * (drive * e.amplitude)
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

/// Drive of unit amplitude at sub-step time `t` of a source whose impulse `-E cos(theta) / omega` follows the
/// envelope `E`, given at `t -+ dt/2`: the impulse's difference over the sub-step,
/// `mean(E) sin(theta) - dE cos(theta) / (2 tan(omega dt / 2))`, scaled so a steady drive is `sin(theta)`.
/// The impulses of the sub-steps telescope, so a drive delivers none once its envelope is back at 0. Past
/// Nyquist (`omega dt >= pi`) the drive is aliased and the `dE` term is dropped: its `tan` nears 0 at multiples of
/// `2 pi`.
fn impulse_drive(omega: f64, phase: f64, t: f64, dt: f64, (lo, hi): (f64, f64)) -> f64 {
	let theta = phase + omega * t;
	let mean = (lo + hi) * 0.5;
	let half = omega * dt / 2.0;
	if lo == hi || half >= std::f64::consts::FRAC_PI_2 {
		return mean * theta.sin();
	}
	mean * theta.sin() - (hi - lo) * theta.cos() / (2.0 * half.tan())
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

	/// The drives of a 1-cycle Gaussian pulse and of a continuous source at phase 0, both detaching, deliver
	/// no net impulse: their sub-steps' values sum to 0 within f32 round-off, at two and three sub-steps.
	#[test]
	fn a_drive_that_ends_leaves_no_impulse() {
		use crate::model::{PulseShape, Stencil};
		let scene = SceneData::blank(16, 16, &crate::model::Scene { damping_edge: 0, ..Default::default() });
		let continuous = Emitter {
			x: 4.0,
			y: 4.0,
			level: 100.0,
			amplitude: 1.0,
			phase_radians: 0.0,
			pulsed: false,
			shape: PulseShape::Gaussian,
			on_width_cycles: 1,
			off_width_cycles: 3,
			off_width_base: 3,
			randomize_off_width: 0,
			detach_after_step: Some(100),
			rng_seed: std::num::NonZeroU32::MIN,
		};
		let pulsed = Emitter { x: 8.0, pulsed: true, detach_after_step: Some(150), ..continuous.clone() };
		for n in [2, 3] {
			let mut sources = Sources::new(StencilCoefficients::of(Stencil::FourthOrder), 2.0, 1.0);
			sources.set_emitters(&[continuous.clone(), pulsed.clone()], &scene);
			let mut out = vec![0.0; sources.table_len()];
			let (mut sum, mut magnitude) = ([0f64; 2], [0f64; 2]);
			for time in 0..200 {
				for k in 0..n {
					sources.values(&mut out, time, k, n, TimeScheme::ModifiedEquation);
					for (j, &v) in out[IMAGE_LEVELS..].iter().enumerate() {
						if v.to_bits() != SKIP_VALUE {
							sum[j] += v as f64;
							magnitude[j] += (v as f64).abs();
						}
					}
				}
			}
			for j in 0..2 {
				assert!(
					magnitude[j] > 0.0 && sum[j].abs() < 1e-6 * magnitude[j],
					"{n} sub-steps, source {j}: {sum:?} of {magnitude:?}"
				);
			}
		}
	}

	#[test]
	fn the_fourth_order_factor_is_the_source_tttt_term() {
		let (omega, n) = (0.4, 2);
		assert_eq!(force_scale(omega, n, TimeScheme::Leapfrog), 0.25);
		assert!((force_scale(omega, n, TimeScheme::ModifiedEquation) - 0.25 * (1.0 - 0.04 / 12.0)).abs() < 1e-15);
		assert!(!live(0.0) && !live(-0.0) && !live(f32::from_bits(SKIP_VALUE)) && live(1e-30));
	}
}
