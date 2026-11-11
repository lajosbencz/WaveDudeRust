//! The run's two clocks and the arithmetic between them, in one place: timeline frame `k` holds the state after
//! step `k * visualize_every` (spec/30). Frames are the position's unit in the views, steps the file's and the
//! engine's; a typed step cannot be handed where a frame is meant.

use std::fmt;

/// A timeline frame index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frame(pub u32);

/// A step index, the engine's time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Step(pub u32);

impl fmt::Display for Frame {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl fmt::Display for Step {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

/// A run's timeline: its steps and the steps between two frames, each at least 1 once normalised.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timeline {
	pub steps: u32,
	pub visualize_every: u32,
}

impl Timeline {
	/// Frames: `ceil(steps / visualize_every)`.
	pub fn frame_count(&self) -> u32 {
		self.steps.div_ceil(self.visualize_every)
	}

	pub fn last_frame(&self) -> Frame {
		Frame(self.frame_count().saturating_sub(1))
	}

	/// The step frame `k` holds the state after.
	pub fn step_of(&self, frame: Frame) -> Step {
		Step(frame.0 * self.visualize_every)
	}

	/// The frame showing `step`, the last frame at or before it.
	pub fn frame_of(&self, step: Step) -> Frame {
		Frame(step.0 / self.visualize_every)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn frames_and_steps_convert_both_ways() {
		let t = Timeline { steps: 10, visualize_every: 4 };
		assert_eq!((t.frame_count(), t.last_frame()), (3, Frame(2)));
		assert_eq!(t.step_of(Frame(2)), Step(8));
		assert_eq!((t.frame_of(Step(8)), t.frame_of(Step(11))), (Frame(2), Frame(2)));
		assert_eq!(Timeline { steps: 1, visualize_every: 1 }.last_frame(), Frame(0));
	}
}
