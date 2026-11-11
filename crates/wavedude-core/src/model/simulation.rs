use super::{Fields, Timeline, Word, ranges};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Word)]
pub enum Stencil {
	/// Isotropic 9-point Laplacian, second order.
	NinePoint,
	/// 7-point DRP-optimised axis stencil, 4th order in space and, by the modified equation, in time.
	#[default]
	FourthOrder,
}

/// Solver settings.
#[derive(Clone, Debug, PartialEq, Fields)]
pub struct Simulation {
	#[field(range = ranges::SIM_STEPS)]
	pub steps: u32,
	/// Steps per timeline frame.
	#[field(range = ranges::SIM_VISUALIZE_EVERY)]
	pub visualize_every: u32,
	pub stencil: Stencil,
	/// Nominal speed in px/step for wavelength estimates in the editor; never an engine input.
	#[field(range = ranges::SIM_WAVE_SPEED)]
	pub wave_speed: f64,
	/// Intensity EMA time constant factor.
	#[field(range = ranges::SIM_TIME_DECAY)]
	pub time_decay: f64,
	/// omega = level/255 * 0.5 * scale.
	#[field(range = ranges::SIM_SOURCE_FREQUENCY_SCALE)]
	pub source_frequency_scale: f64,
	#[field(range = ranges::SIM_INTENSITY_SCALE)]
	pub intensity_scale: f64,
}

impl Default for Simulation {
	fn default() -> Self {
		Self {
			steps: 2500,
			visualize_every: 2,
			stencil: Stencil::FourthOrder,
			wave_speed: 1.0,
			time_decay: 10.0,
			source_frequency_scale: 2.0,
			intensity_scale: 1.0,
		}
	}
}

impl Simulation {
	pub fn timeline(&self) -> Timeline {
		Timeline { steps: self.steps, visualize_every: self.visualize_every }
	}

	/// Timeline frames of a run.
	pub fn frame_count(&self) -> u32 {
		self.timeline().frame_count()
	}
}
