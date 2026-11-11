//! The layout of the logic gates (XOR, AND, OR): spec 21's design procedure as formulas.
//!
//! All lengths in cells (pixels), angles in radians. The part frame has x' along the gate from the
//! inputs (-x') to the outputs (+x'), y' across, arm A at y' < 0, arm B at y' > 0. The mode solver
//! is [`ModeSolver`] for the simulation's stencil and sub-steps, so the design holds for the engine
//! that runs it (on a scene whose fastest pixel has index 1).

use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};
use std::sync::{Arc, Mutex, OnceLock};

use crate::model::{GateKind, INDEX_MAX, LogicGate, PartBase, Point, Simulation};
use crate::scene::modes::ModeSolver;
use crate::scene::shapes::{Frame, Vec2, round_digits};

/// Index step of the phase section.
pub const PHASE_STEP: f64 = 0.1;
/// Largest part index.
pub const MAX_INDEX: f64 = INDEX_MAX;
/// Bend radiation exponent: `(2/3) gamma^3 R / beta^2`.
pub const BEND_EXP: f64 = 11.0;
/// Narrowest absorbing band, in wavelengths.
pub const MIN_ABSORBER: f64 = 0.5;
/// Blue level at full absorber share.
pub const ABSORPTION: u32 = 64;
/// Largest share of pi/4 the two S-bends may couple.
pub const BEND_SHARE_LIMIT: f64 = 0.6;
/// Measured extra coupling taken off the straight coupler, radians.
pub const COUPLING_EXCESS: f64 = 0.07;
/// Simpson intervals per S-bend.
pub const BEND_SAMPLES: usize = 24;

/// The S-bend's shape: `t - sin(2 pi t)/(2 pi)`, 0 to 1 with zero slope at both ends.
pub fn bend_share(t: f64) -> f64 {
	t - (TAU * t).sin() / TAU
}

/// The phase shift of a kind in degrees: wave B ahead of wave A.
pub fn phase_shift(kind: GateKind) -> f64 {
	match kind {
		GateKind::Xor => 90.0,
		GateKind::And => -90.0,
		GateKind::Or => 30.0,
	}
}

/// The design wavelength in cells at index 1 for level `frequency` and frequency scale `scale`:
/// `round_even(clamp(2 pi / (F/255 * 0.5 * s), 6, 200), 3)`; None when not finite and positive.
pub fn gate_wavelength(frequency: f64, scale: f64) -> Option<f64> {
	let cycles = frequency / 255.0 * 0.5 * scale / TAU;
	let raw = if cycles > 0.0 { 1.0 / cycles } else { f64::NAN };
	(raw.is_finite() && raw > 0.0).then(|| round_digits(raw.clamp(6.0, 200.0), 3))
}

/// A gate's layout. Integers are cells; see spec 21 for every symbol.
#[derive(Clone, Debug, PartialEq)]
pub struct GateDesign {
	/// lam: wavelength at index 1.
	pub wavelength: f64,
	pub core_index: f64,
	pub cladding_index: f64,
	/// `n_p`: the phase arm's core index.
	pub phase_index: f64,
	/// The phase section lies on arm A (else B).
	pub phase_on_a: bool,
	/// w
	pub core_width: i64,
	/// c: clear cladding thickness.
	pub cladding: i64,
	/// `a_min`
	pub absorber_min: i64,
	/// beta of the single guide in its band (rad/cell).
	pub beta: f64,
	/// beta of the phase arm.
	pub phase_beta: f64,
	/// gamma: cladding decay (1/cell).
	pub gamma: f64,
	/// R: smallest S-bend radius.
	pub radius: f64,
	/// D: coupled spacing, centre to centre (even).
	pub coupled: i64,
	/// P: port spacing (even).
	pub port_spacing: i64,
	/// kappa of the straight coupler (rad/cell).
	pub kappa: f64,
	/// B: coupling of one S-bend (rad).
	pub bend_kappa: f64,
	pub bend_len: i64,
	pub coupler_len: i64,
	/// `L_reg`
	pub phase_region: i64,
	/// `L_ph`
	pub phase_len: i64,
	pub input_len: i64,
	pub lead_len: i64,
	pub output_len: i64,
	pub ramp_len: i64,
	/// Lambda: total length.
	pub length: i64,
}

impl GateDesign {
	/// The design procedure of spec 21 for wavelength `lam`, indices, requested port spacing
	/// `port_in` and phase shift `phi` (degrees); None when nothing guides.
	pub fn design(lam: f64, n_c: f64, n_cl: f64, port_in: i64, phi: f64, solver: &ModeSolver) -> Option<Self> {
		debug_assert!(lam.is_finite() && lam > 0.0, "wavelength {lam}");
		if n_c <= n_cl {
			return None;
		}
		let om = solver.omega_term(lam);
		let na = (n_c * n_c - n_cl * n_cl).sqrt();
		let w = ((0.9 * lam / (2.0 * na)).floor() as i64).max(2);
		let mode0 = solver.single(lam, om, n_c, n_cl, w, 0)?;
		let gamma = mode0.cladding_decay(om, n_cl);
		let c = ((3.0 / gamma).ceil() as i64).max(2);
		let a_min = ((MIN_ABSORBER * lam).ceil() as i64).max(2);
		let o = c + a_min;
		let lowered = n_c + PHASE_STEP > MAX_INDEX + 1e-9;
		let b_leads = phi >= 0.0;
		let phase_on_a = b_leads == lowered;
		let n_p = round_digits(if lowered { n_c - PHASE_STEP } else { n_c + PHASE_STEP }, 2);
		let beta = solver.single(lam, om, n_c, n_cl, w, o).map_or(mode0.beta, |m| m.beta);
		let phase_beta =
			if n_p > n_cl { solver.single(lam, om, n_p, n_cl, w, o).map_or(beta, |m| m.beta) } else { beta };
		let dbeta = (phase_beta - beta).abs();
		if dbeta < 1e-6 {
			return None;
		}
		let phase_region = ((FRAC_PI_2 / dbeta).round_ties_even() as i64).max(1);
		let phase_len = phase_region.min((phi.abs() * PI / 180.0 / dbeta).round_ties_even() as i64);
		let radius = BEND_EXP * 1.5 * beta * beta / gamma.powi(3);

		let kappa_at = |dist: f64| solver.kappa_at(dist, om, n_c, n_cl, w, o);
		let mut gap = ((1.0 / gamma).round_ties_even() as i64).max(2);
		gap += (w + gap) % 2;
		let (coupled, port_spacing, bend_len, kappa, bend_kappa) = loop {
			let coupled = w + gap;
			let mut p = port_in.max((coupled + w + 2 * c + 2).max((w as f64 + 2.0 * (c + a_min) as f64).ceil() as i64));
			p += p % 2;
			let bend_len = ((TAU * radius * (p - coupled) as f64 / 2.0).sqrt().ceil() as i64).max(1);
			let kappa = kappa_at(coupled as f64);
			let bend_kappa = bend_kappa(coupled, p, bend_len, &kappa_at);
			if 2.0 * bend_kappa <= BEND_SHARE_LIMIT * FRAC_PI_4 || gap >= 2 * c {
				break (coupled, p, bend_len, kappa, bend_kappa);
			}
			gap += 2;
		};
		let coupler_len = if kappa > 0.0 {
			(((FRAC_PI_4 - COUPLING_EXCESS - 2.0 * bend_kappa) / kappa).round_ties_even() as i64).max(0)
		} else {
			0
		};
		let input_len = (lam / 2.0).round_ties_even() as i64;
		let lead_len = lam.round_ties_even() as i64;
		let output_len = (3.0 * lam).round_ties_even() as i64;
		let ramp_len = (2.0 * lam).round_ties_even() as i64;
		let length = input_len + lead_len + phase_region + lead_len + 2 * bend_len + coupler_len + output_len;
		Some(Self {
			wavelength: lam,
			core_index: n_c,
			cladding_index: n_cl,
			phase_index: n_p,
			phase_on_a,
			core_width: w,
			cladding: c,
			absorber_min: a_min,
			beta,
			phase_beta,
			gamma,
			radius,
			coupled,
			port_spacing,
			kappa,
			bend_kappa,
			bend_len,
			coupler_len,
			phase_region,
			phase_len,
			input_len,
			lead_len,
			output_len,
			ramp_len,
			length,
		})
	}

	/// The design of a normalised gate in a simulation, cached by its inputs.
	pub fn for_gate(gate: &LogicGate, sim: &Simulation) -> Option<Arc<Self>> {
		let lam = gate_wavelength(gate.design_frequency, sim.source_frequency_scale)?;
		let (n_c, n_cl) = (gate.core_index, gate.cladding_index);
		let port_in = gate.port_spacing as i64;
		let phi = phase_shift(gate.kind);
		let solver = ModeSolver::for_simulation(sim);
		let key = (lam.to_bits(), n_c.to_bits(), n_cl.to_bits(), port_in, phi.to_bits(), solver);
		static CACHE: OnceLock<Mutex<HashMap<DesignKey, Option<Arc<GateDesign>>>>> = OnceLock::new();
		let cache = CACHE.get_or_init(Default::default);
		if let Some(hit) = cache.lock().unwrap().get(&key) {
			return hit.clone();
		}
		let design = Self::design(lam, n_c, n_cl, port_in, phi, &solver).map(Arc::new);
		let mut map = cache.lock().unwrap();
		if map.len() >= 64 {
			map.clear();
		}
		map.insert(key, design.clone());
		design
	}

	/// `A_w`: absorber band width, `P/2 - w/2 - c`.
	pub fn absorber_width(&self) -> f64 {
		self.port_spacing as f64 / 2.0 - self.core_width as f64 / 2.0 - self.cladding as f64
	}

	/// Half the unit's outer width: P.
	pub fn half_width(&self) -> f64 {
		self.port_spacing as f64
	}

	/// `x_L`: the input end.
	pub fn left(&self) -> f64 {
		-(self.length as f64) / 2.0
	}

	/// `x_in`: the input ports.
	pub fn input_x(&self) -> f64 {
		self.left() + self.input_len as f64
	}

	/// [`x_ps`, `x_pe)`: the phase section.
	pub fn phase_section(&self) -> (f64, f64) {
		let start = self.input_x() + self.lead_len as f64;
		(start, start + self.phase_len as f64)
	}

	/// `x_b1`: the S-bend in starts.
	pub fn bend_in(&self) -> f64 {
		self.input_x() + 2.0 * self.lead_len as f64 + self.phase_region as f64
	}

	/// `x_c0`: the coupler starts.
	pub fn coupler_start(&self) -> f64 {
		self.bend_in() + self.bend_len as f64
	}

	/// `x_c1`: the coupler ends, the S-bend out starts.
	pub fn coupler_end(&self) -> f64 {
		self.coupler_start() + self.coupler_len as f64
	}

	/// `x_out`: the output ports.
	pub fn output_x(&self) -> f64 {
		self.length as f64 / 2.0 - self.output_len as f64 + (self.wavelength / 2.0).round_ties_even()
	}

	/// `x_r`: a switched-off output's absorber starts to rise.
	pub fn ramp_start(&self) -> f64 {
		self.length as f64 / 2.0 - self.ramp_len as f64
	}

	/// The arm's offset h from the axis at x' and its slope h'.
	pub fn arm_offset_and_slope(&self, x: f64) -> (f64, f64) {
		let far = self.port_spacing as f64 / 2.0;
		let near = self.coupled as f64 / 2.0;
		let bend_in = self.bend_in();
		let coupler_start = self.coupler_start();
		let bend_out = self.coupler_end();
		let l = self.bend_len as f64;
		if x <= bend_in || x >= bend_out + l {
			return (far, 0.0);
		}
		if x >= coupler_start && x <= bend_out {
			return (near, 0.0);
		}
		let (t, sign) = if x < coupler_start { ((x - bend_in) / l, 1.0) } else { (1.0 - (x - bend_out) / l, -1.0) };
		let offset = far - (far - near) * bend_share(t);
		let slope = -sign * (far - near) * (1.0 - (TAU * t).cos()) / l;
		(offset, slope)
	}

	/// Input A, input B, upper output, lower output in the part frame.
	pub fn port_points(&self) -> [Vec2; 4] {
		let half = self.port_spacing as f64 / 2.0;
		let (xi, xo) = (self.input_x(), self.output_x());
		[Vec2::new(xi, -half), Vec2::new(xi, half), Vec2::new(xo, -half), Vec2::new(xo, half)]
	}
}

type DesignKey = (u64, u64, u64, i64, u64, ModeSolver);

fn bend_kappa(coupled: i64, spacing: i64, len: i64, kappa_at: &impl Fn(f64) -> f64) -> f64 {
	let (d, p) = (coupled as f64, spacing as f64);
	let sum: f64 = (0..=BEND_SAMPLES)
		.map(|i| {
			let weight = if i == 0 || i == BEND_SAMPLES {
				1.0
			} else if i % 2 == 1 {
				4.0
			} else {
				2.0
			};
			weight * kappa_at(p - (p - d) * bend_share(i as f64 / BEND_SAMPLES as f64))
		})
		.sum();
	sum * len as f64 / (3.0 * BEND_SAMPLES as f64)
}

/// The four ports of a gate in canvas pixels, `(floor(cx), floor(cy))`: input A, input B, upper
/// output, lower output; None without a design.
pub fn ports(base: &PartBase, gate: &LogicGate, sim: &Simulation) -> Option<[Point; 4]> {
	GateDesign::for_gate(gate, sim).map(|design| ports_of(&design, base))
}

/// The four ports of `design` placed at `base` (position and angle), in canvas pixels.
pub fn ports_of(design: &GateDesign, base: &PartBase) -> [Point; 4] {
	let frame = Frame::of(base);
	design.port_points().map(|p| {
		let c = frame.to_canvas(p);
		Point::new(c.x.floor() as i32, c.y.floor() as i32)
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Stencil;

	fn solver() -> ModeSolver {
		ModeSolver::original()
	}

	#[test]
	fn example_18_default_gate() {
		let lam = gate_wavelength(50.0, 2.0).unwrap();
		assert_eq!(lam, 32.044);
		let d = GateDesign::design(lam, 2.55, 1.45, 2, 90.0, &solver()).unwrap();
		assert_eq!((d.core_width, d.cladding, d.absorber_min), (6, 10, 17));
		assert!((d.gamma - 0.3048883).abs() < 1e-6);
		assert_eq!((d.phase_index, d.phase_on_a), (2.45, true));
		assert_eq!((d.phase_region, d.phase_len), (88, 88));
		assert!((d.radius - 101.196).abs() < 1e-3, "{}", d.radius);
		assert_eq!((d.coupled, d.port_spacing, d.bend_len, d.coupler_len), (12, 60, 124, 32));
		assert!((d.kappa - 0.0085384994).abs() < 1e-9);
		// spec 21 gives 0.44281 from a Python port whose row rounding is Python's correctly rounded
		// round(); the C# Math.Round(x, 2) scales by 100 first, which moves a few near-tie rows
		assert!((2.0 * d.bend_kappa - 0.44330).abs() < 1e-5, "{}", 2.0 * d.bend_kappa);
		assert_eq!((d.input_len, d.lead_len, d.output_len, d.ramp_len, d.length), (16, 32, 96, 64, 544));
		assert_eq!(d.absorber_width(), 17.0);
		assert_eq!((d.left(), d.input_x(), d.phase_section()), (-272.0, -256.0, (-224.0, -136.0)));
		assert_eq!((d.bend_in(), d.coupler_start(), d.coupler_end()), (-104.0, 20.0, 52.0));
		assert_eq!((d.output_x(), d.ramp_start()), (192.0, 208.0));
		let and = GateDesign::design(lam, 2.55, 1.45, 2, -90.0, &solver()).unwrap();
		assert!(!and.phase_on_a);
		let or = GateDesign::design(lam, 2.55, 1.45, 2, 30.0, &solver()).unwrap();
		assert_eq!((or.phase_on_a, or.phase_len), (true, 29));
	}

	#[test]
	fn other_designs_table() {
		// lam, n_c, n_cl, P_in, w, c, gap, P, L_b, L_c, L_reg, L_ph, Lambda, n_p, phase on A.
		// Spec 21's table, except where its Python port's row rounding (Python's round()) differs from
		// the C# Math.Round(x, 2) kept here: lam 12 L_b 309 -> 310, Lambda 800 -> 802; lam 32 L_b 608 ->
		// 609, L_reg/L_ph 91 -> 90, Lambda 1673 -> 1674; lam 16.022 L_reg/L_ph 46 -> 44, Lambda 411 -> 409.
		let table = [
			(12.0, 1.6, 1.45, 64, 7, 12, 7, 64, 310, 81, 35, 35, 802, 1.7, false),
			(16.0, 1.6, 1.45, 64, 10, 15, 8, 64, 326, 93, 46, 46, 879, 1.7, false),
			(32.0, 1.6, 1.45, 64, 21, 30, 15, 120, 609, 190, 90, 90, 1674, 1.7, false),
			(16.022, 2.0, 1.45, 2, 5, 8, 5, 40, 107, 63, 44, 44, 409, 2.1, false),
			(32.044, 2.55, 1.45, 2, 6, 10, 6, 60, 124, 32, 88, 88, 544, 2.45, true),
		];
		for (lam, nc, ncl, p_in, w, c, gap, p, lb, lc, lreg, lph, len, np, on_a) in table {
			let d = GateDesign::design(lam, nc, ncl, p_in, 90.0, &solver()).unwrap();
			let got = (d.core_width, d.cladding, d.coupled - d.core_width, d.port_spacing, d.bend_len, d.coupler_len);
			assert_eq!(got, (w, c, gap, p, lb, lc), "lam {lam}");
			assert_eq!(
				(d.phase_region, d.phase_len, d.length, d.phase_index, d.phase_on_a),
				(lreg, lph, len, np, on_a)
			);
		}
	}

	#[test]
	fn ports_of_example_18() {
		let xor = GateDesign::design(32.044, 2.55, 1.45, 2, 90.0, &solver()).unwrap();
		let base = PartBase { x: 396, y: 163, ..PartBase::default() };
		let p = ports_of(&xor, &base);
		assert_eq!(p, [Point::new(140, 133), Point::new(140, 193), Point::new(588, 133), Point::new(588, 193)]);
		let and = GateDesign::design(32.044, 2.55, 1.45, 2, -90.0, &solver()).unwrap();
		let base = PartBase { x: 936, y: 103, ..PartBase::default() };
		assert_eq!(ports_of(&and, &base)[3], Point::new(1128, 133));
	}

	#[test]
	fn default_gate_with_this_engines_sub_steps() {
		let sim = Simulation::default();
		let d = GateDesign::for_gate(&LogicGate::default(), &sim).unwrap();
		assert!(d.length < 650, "{d:?}");
		assert!((d.phase_index, d.phase_on_a) == (2.45, true) && d.port_spacing % 2 == 0 && d.coupled % 2 == 0);
		let level_100 = LogicGate { design_frequency: 100.0, core_index: 2.0, ..LogicGate::default() };
		let d = GateDesign::for_gate(&level_100, &sim).unwrap();
		assert!(d.length < 500 && !d.phase_on_a && d.phase_index == 2.1, "{d:?}");
		let short = GateDesign::design(12.0, 1.6, 1.45, 64, 90.0, &ModeSolver::for_simulation(&sim)).unwrap();
		let long = GateDesign::design(32.0, 1.6, 1.45, 64, 90.0, &ModeSolver::for_simulation(&sim)).unwrap();
		assert!(short.core_width < long.core_width && long.length > short.length && long.port_spacing > 64);
	}

	#[test]
	fn no_design_without_guidance() {
		assert!(GateDesign::design(16.0, 1.45, 1.45, 64, 90.0, &solver()).is_none());
		let gate = LogicGate { core_index: 1.4, ..LogicGate::default() };
		assert!(ports(&PartBase::default(), &gate, &Simulation::default()).is_none());
	}

	#[test]
	fn nine_point_design_exists() {
		let s = ModeSolver::new(Stencil::NinePoint, 2);
		let d = GateDesign::design(32.044, 2.55, 1.45, 2, 90.0, &s).unwrap();
		assert!(d.coupler_len > 0 && d.port_spacing % 2 == 0 && d.coupled % 2 == 0);
		assert!(d.length > 300 && d.length < 1500, "{}", d.length);
	}
}
