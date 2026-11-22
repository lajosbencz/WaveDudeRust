use std::sync::Arc;

use super::{PartShape, Vec2, absorption_profile, round_digits};
use crate::model::{LogicGate, Simulation};
use crate::scene::gate_design::{ABSORPTION, GateDesign};

/// A logic gate: two arms mirrored about the axis, each a core in its cladding, cut square at
/// both ends. Arm distance is first order (`| |y'| - h(x') | / sqrt(1 + h'^2)`), so the depth is
/// not a distance; the gate reports inner edges everywhere (every pixel near it is supersampled).
pub(super) struct GateShape {
	design: Arc<GateDesign>,
	half_length: f64,
	core: f64,
	outer: f64,
	end_coating: f64,
	input_coated: bool,
	output_coated: bool,
	upper_active: bool,
	lower_active: bool,
}

impl GateShape {
	/// None without a design (no guided mode, no wavelength).
	pub(super) fn new(gate: &LogicGate, sim: &Simulation) -> Option<Self> {
		let design = GateDesign::for_gate(gate, sim)?;
		let core = design.core_width as f64 / 2.0;
		Some(Self {
			half_length: design.length as f64 / 2.0,
			core,
			outer: core + design.cladding as f64 + design.absorber_width(),
			end_coating: gate.coating,
			input_coated: gate.input_coated,
			output_coated: gate.output_coated,
			upper_active: gate.upper_output_active,
			lower_active: gate.lower_output_active,
			design,
		})
	}

	/// Distance from the nearer arm's centre line, and whether that arm is B (y' > 0).
	fn nearest(&self, p: Vec2) -> (f64, bool) {
		let (offset, slope) = self.design.arm_offset_and_slope(p.x);
		let cos = 1.0 / (1.0 + slope * slope).sqrt();
		((p.y.abs() - offset).abs() * cos, p.y > 0.0)
	}
}

impl PartShape for GateShape {
	fn depth(&self, p: Vec2) -> f64 {
		(self.half_length - p.x.abs()).min(self.outer - self.nearest(p).0)
	}

	/// Core, the phase section on its arm, or cladding; within the end coating of a coated end `round_even(sqrt(n), 2)`.
	fn body_index_at(&self, p: Vec2) -> f64 {
		let d = &self.design;
		let (distance, arm_b) = self.nearest(p);
		let (start, end) = d.phase_section();
		let index = if distance > self.core {
			d.cladding_index
		} else if arm_b != d.phase_on_a && p.x >= start && p.x < end {
			d.phase_index
		} else {
			d.core_index
		};
		let coated = if p.x < 0.0 { self.input_coated } else { self.output_coated };
		if coated && self.end_coating > 0.0 && self.half_length - p.x.abs() < self.end_coating {
			round_digits(index.sqrt(), 2)
		} else {
			index
		}
	}

	fn extent(&self) -> f64 {
		let h = self.design.half_width();
		(self.half_length * self.half_length + h * h).sqrt()
	}

	fn inner_edge_distance(&self, _p: Vec2) -> f64 {
		0.0
	}

	fn absorption(&self) -> u32 {
		ABSORPTION
	}

	fn absorption_ramp(&self) -> f64 {
		self.design.absorber_width().ceil().max(1.0)
	}

	/// Side band: `(1 - inside/A_w)^2` toward the cladding's rim; a switched-off output's arm rises
	/// as t^2 over the output ramp to the right end.
	fn absorption_share(&self, p: Vec2, _depth: f32, _ramp: f64) -> f64 {
		let (distance, arm_b) = self.nearest(p);
		let inside = self.outer - distance;
		if inside < 0.0 || p.x.abs() > self.half_length {
			return 0.0;
		}
		let d = &self.design;
		let side = absorption_profile(1.0 - inside / d.absorber_width());
		let active = if arm_b { self.lower_active } else { self.upper_active };
		let output = if active { 0.0 } else { absorption_profile((p.x - d.ramp_start()) / d.ramp_len as f64) };
		side.max(output)
	}
}
