//! An indeterminate spinner: a three-quarter ring turning once a second.

use std::time::{Duration, Instant};

use freya::prelude::*;

use crate::paint::stroke;

/// Angle of the ring in degrees after `elapsed`, one turn per `period`.
pub fn spinner_angle(elapsed: Duration, period: Duration) -> f32 {
	let t = elapsed.as_secs_f64() / period.as_secs_f64();
	(t.fract() * 360.) as f32
}

/// An animated three-quarter ring of `size` logical pixels. Not hit-testable.
#[derive(Clone, PartialEq)]
pub struct Spinner {
	size: f32,
	color: Color,
	width: f32,
}

impl Default for Spinner {
	fn default() -> Self {
		Self::new()
	}
}

impl Spinner {
	pub fn new() -> Self {
		Self { size: 32., color: Color::WHITE, width: 3. }
	}

	pub fn size(mut self, size: f32) -> Self {
		self.size = size;
		self
	}

	pub fn color(mut self, color: Color) -> Self {
		self.color = color;
		self
	}

	pub fn stroke_width(mut self, width: f32) -> Self {
		self.width = width;
		self
	}
}

impl Component for Spinner {
	fn render(&self) -> impl IntoElement {
		let mut tick = use_state(|| 0u64);
		let start = use_hook(Instant::now);
		use_hook(move || {
			spawn(async move {
				loop {
					timer(Duration::from_millis(16)).await;
					*tick.write() += 1;
				}
			});
		});
		let _ = tick.read();
		let angle = spinner_angle(start.elapsed(), Duration::from_secs(1));
		let (size, color, width) = (self.size, self.color, self.width);
		rect().interactive(Interactive::No).width(Size::px(size)).height(Size::px(size)).background(
			RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
				let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
				let mut paint = stroke(color, width);
				paint.set_anti_alias(true);
				let inset = width / 2. + 0.5;
				let oval = freya::engine::prelude::SkRect::new(inset, inset, size - inset, size - inset);
				canvas.draw_arc(oval, angle - 90., 270., false, &paint);
			}),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn one_turn_per_period() {
		let p = Duration::from_secs(1);
		assert_eq!(spinner_angle(Duration::ZERO, p), 0.);
		assert!((spinner_angle(Duration::from_millis(250), p) - 90.).abs() < 1e-3);
		assert!((spinner_angle(Duration::from_millis(1500), p) - 180.).abs() < 1e-3);
	}
}
